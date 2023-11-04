#[allow(unused_imports)]
use cached::proc_macro::io_cached;
#[allow(unused_imports)]
use cached::AsyncRedisCache;
use google_youtube3::{
    api::{self, PlaylistItem},
    hyper, hyper_rustls, YouTube,
};
use std::{collections::HashMap, str::FromStr};

use async_trait::async_trait;
use eyre::eyre;
use log::{debug, info, warn};
use regex::Regex;
use reqwest::Url;
use rss::{
    extension::itunes::{ITunesChannelExtensionBuilder, ITunesItemExtensionBuilder},
    Channel, ChannelBuilder, GuidBuilder, ImageBuilder, Item, ItemBuilder,
};
use tokio::process::Command;

use crate::{
    configs::{conf, Conf, ConfName},
    provider,
};

use super::MediaProviderV2;

pub(super) struct YoutubeProvider {}

enum IdType {
    Playlist(String),
    Channel(String),
}

#[async_trait]
impl MediaProviderV2 for YoutubeProvider {
    fn media_url_regexes(&self) -> Vec<Regex> {
        return vec![regex::Regex::new(r"^(https?://)?(www\.youtube\.com|youtu\.be)/.+$").unwrap()];
    }
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        let youtube_api_key = conf().get(ConfName::YoutubeApiKey).ok();

        match youtube_api_key {
            Some(api_key) => {
                info!(
                    "starting youtube feed generation for {} with API key",
                    channel_url
                );
                let mut feed_builder = provider::build_default_rss_structure();

                let id = match channel_url.path() {
                    path if path.starts_with("/playlist") => {
                        let playlist_id = channel_url
                            .query_pairs()
                            .find(|(key, _)| key == "list")
                            .map(|(_, value)| value)
                            .ok_or_else(|| {
                                eyre::eyre!("Failed to parse playlist ID from URL: {}", channel_url)
                            })?;
                        IdType::Playlist(playlist_id.into())
                    }
                    path if path.starts_with("/channel/")
                        || path.starts_with("/user/")
                        || path.starts_with("/c/")
                        || path.starts_with("/c/")
                        || path.starts_with("/@") =>
                    {
                        let url = find_yt_channel_url_with_c_id(&channel_url).await?;
                        let channel_id = url.path_segments().unwrap().last().unwrap();
                        IdType::Channel(channel_id.into())
                    }
                    _ => return Err(eyre!("unsupported youtube url")),
                };

                let mut video_items = fetch_from_api(id, api_key).await?;

                feed_builder.description(video_items.0.description);
                feed_builder.title(video_items.0.title);
                feed_builder.language(video_items.0.language.take());
                let mut image_builder = ImageBuilder::default();
                image_builder.url(
                    video_items
                        .0
                        .itunes_ext
                        .clone()
                        .and_then(|it| it.image)
                        .unwrap_or_default(),
                );
                feed_builder.image(Some(image_builder.build()));
                feed_builder.itunes_ext(video_items.0.itunes_ext.take());
                feed_builder.link(video_items.0.link);

                feed_builder.items(video_items.1);

                return Ok(feed_builder.build().to_string());
            }
            None => {
                info!(
                    "starting youtube feed generation for {} using atom feed",
                    channel_url
                );
                let feed = match channel_url.path() {
                    path if path.starts_with("/playlist") => {
                        feed_url_for_yt_playlist(&channel_url).await
                    }
                    path if path.starts_with("/feeds/") => feed_url_for_yt_atom(&channel_url).await,
                    path if path.starts_with("/channel/") => {
                        feed_url_for_yt_channel(&channel_url).await
                    }
                    path if path.starts_with("/user/") => {
                        feed_url_for_yt_channel(&channel_url).await
                    }
                    path if path.starts_with("/c/") => feed_url_for_yt_channel(&channel_url).await,
                    path if path.starts_with("/@") => feed_url_for_yt_channel(&channel_url).await,
                    _ => Err(eyre!("unsupported youtube url")),
                }?;
                let response = reqwest::get(feed).await?.text().await?;
                return Ok(response);
            }
        }
    }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        get_youtube_stream_url(media_url).await
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        let youtube_whitelist = vec![
            regex::Regex::new(r"^(https://)?.*\.youtube\.com/").unwrap(),
            regex::Regex::new(r"^(https://)?youtube\.com/").unwrap(),
            regex::Regex::new(r"^(https://)?youtu\.be/").unwrap(),
            regex::Regex::new(r"^(https://)?.*\.youtu\.be/").unwrap(),
            regex::Regex::new(r"^(https://)?.*\.googlevideo\.com/").unwrap(),
            regex::Regex::new(r"^http://localhost:15000/youtube").unwrap(),
            regex::Regex::new(r"^http://podtube(:[0-9]+)?/youtube").unwrap(),
        ];

        #[cfg(not(test))]
        return youtube_whitelist;
        #[cfg(test)] //this will allow test to use localhost ad still work
        return [
            youtube_whitelist,
            vec![regex::Regex::new(r"^http://127\.0\.0\.1:9870").unwrap()],
        ]
        .concat();
    }

    fn new() -> Self {
        YoutubeProvider {}
    }
}

async fn fetch_from_api(id: IdType, api_key: String) -> eyre::Result<(Channel, Vec<Item>)> {
    match id {
        IdType::Playlist(id) => {
            info!("fetching playlist {}", id);
            let mut playlist = fetch_playlist(id, &api_key).await?;

            let playlist_id = playlist.id.take().ok_or(eyre!("playlist has no id"))?;

            let rss_channel = build_channel_from_playlist(playlist);

            let items = fetch_playlist_items(&playlist_id, &api_key).await?;

            let duration_map = create_duration_url_map(&items, &api_key).await?;

            let rss_items = build_channel_items_from_playlist(items, duration_map);

            return Ok((rss_channel, rss_items));
        }
        IdType::Channel(id) => {
            info!("fetching channel {}", id);
            let mut channel = fetch_channel(id, &api_key).await?;

            let upload_playlist = channel
                .content_details
                .take()
                .ok_or(eyre!("content_details is None"))?
                .related_playlists
                .ok_or(eyre!("related_playlists is None"))?
                .uploads
                .ok_or(eyre!("uploads is None"))?;

            let rss_channel = build_channel_from_yt_channel(channel);

            let items = fetch_playlist_items(&upload_playlist, &api_key).await?;

            let duration_map = create_duration_url_map(&items, &api_key).await?;

            let rss_items = build_channel_items_from_playlist(items, duration_map);

            return Ok((rss_channel, rss_items));
        }
    };
}

fn build_channel_from_yt_channel(channel: api::Channel) -> Channel {
    let mut channel_builder = ChannelBuilder::default();
    let mut itunes_channel_builder = ITunesChannelExtensionBuilder::default();

    if let Some(mut snippet) = channel.snippet {
        channel_builder.description(snippet.description.take().unwrap_or("".to_owned()));
        channel_builder.title(snippet.title.take().unwrap_or("".to_owned()));
        channel_builder.language(snippet.default_language.take());
        if let Some(mut thumb) = get_thumb_from_channel_snippet(snippet) {
            itunes_channel_builder.image(thumb.url.take());
        }
        itunes_channel_builder.explicit(Some("no".to_owned()));
    }
    channel_builder.link(format!(
        "https://www.youtube.com/channel/{}",
        channel.id.unwrap_or_default()
    ));

    channel_builder.itunes_ext(Some(itunes_channel_builder.build()));
    channel_builder.build()
}

fn get_thumb_from_playlist_snippet(snippet: api::PlaylistSnippet) -> Option<api::Thumbnail> {
    snippet.thumbnails.and_then(|thumbs| {
        thumbs
            .default
            .or(thumbs.standard)
            .or(thumbs.medium)
            .or(thumbs.high)
            .or(thumbs.maxres)
    })
}

fn get_thumb_from_playlist_item_snippet(
    snippet: api::PlaylistItemSnippet,
) -> Option<api::Thumbnail> {
    snippet.thumbnails.and_then(|thumbs| {
        thumbs
            .default
            .or(thumbs.standard)
            .or(thumbs.medium)
            .or(thumbs.high)
            .or(thumbs.maxres)
    })
}

fn get_thumb_from_channel_snippet(snippet: api::ChannelSnippet) -> Option<api::Thumbnail> {
    snippet.thumbnails.and_then(|thumbs| {
        thumbs
            .default
            .or(thumbs.standard)
            .or(thumbs.medium)
            .or(thumbs.high)
            .or(thumbs.maxres)
    })
}

async fn fetch_channel(id: String, api_key: &str) -> eyre::Result<api::Channel> {
    let hub = get_youtube_hub();
    let channel_request = hub
        .channels()
        .list(&vec!["snippet".into(), "contentDetails".into()])
        .max_results(1)
        .add_id(&id)
        .param("key", &api_key);
    let result = channel_request.doit().await?;
    let channel = result
        .1
        .items
        .ok_or(eyre!("youtube returned no channel with id {:?}", id))?
        .first()
        .ok_or(eyre!("youtube returned no channel with id {:?}", id))?
        .clone();
    Ok(channel)
}

#[derive(Debug, Clone)]
struct VideoExtraInfo {
    duration: iso8601_duration::Duration,
}

async fn create_duration_url_map(
    items: &Vec<PlaylistItem>,
    api_key: &str,
) -> Result<HashMap<String, VideoExtraInfo>, eyre::Error> {
    let ids_batches = items.chunks(50).map(|c| {
        c.into_iter()
            .filter_map(|i| i.snippet.clone()?.resource_id?.video_id)
    });

    let hub = get_youtube_hub();
    let videos_requests = ids_batches.map(|batch| {
        let mut video_info_request = hub
            .videos()
            .list(&vec!["contentDetails".to_owned()])
            .param("key", &api_key);

        for video_id in batch {
            video_info_request = video_info_request.add_id(&video_id);
        }
        return video_info_request.doit();
    });

    info!(
        "fetching video info for {} videos in {} batches",
        items.len(),
        videos_requests.len()
    );

    let video_infos = futures::future::join_all(videos_requests)
        .await
        .into_iter()
        .flatten()
        .map(|r| {
            r.0.status()
                .is_success()
                .then(|| r.1.items.unwrap())
                .ok_or_else(|| eyre!("error fetching video info {:?}", r.0))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .filter_map(|v| {
            Some((
                v.id?,
                VideoExtraInfo {
                    duration: iso8601_duration::Duration::parse(&v.content_details?.duration?)
                        .unwrap(),
                },
            ))
        })
        .collect::<HashMap<_, _>>();

    Ok(video_infos)
}

fn build_channel_items_from_playlist(
    items: Vec<PlaylistItem>,
    videos_infos: HashMap<String, VideoExtraInfo>,
) -> Vec<Item> {
    let rss_item: Vec<Item> = items
        .into_iter()
        .filter_map(|item| {
            let mut snippet = item.snippet?;
            let title = snippet.title.take().unwrap_or("".to_owned());
            let description = snippet.description.take().unwrap_or("".to_owned());
            let video_id = snippet.resource_id.take()?.video_id?;
            let url = Url::parse(&format!("https://www.youtube.com/watch?v={}", &video_id)).ok()?;
            let mut item_builder = ItemBuilder::default();
            item_builder.title(Some(title));
            item_builder.description(Some(description.clone()));
            item_builder.link(Some(url.to_string()));
            item_builder.guid(Some(GuidBuilder::default().value(url.to_string()).build()));
            item_builder.pub_date(
                snippet
                    .published_at
                    .and_then(|pub_date| Some(pub_date.to_rfc2822().to_string())),
            );
            item_builder.author(snippet.channel_title.take());
            let video_infos = videos_infos.get(&video_id).or_else(|| {
                warn!("no duration found for {:?}", &video_id);
                None
            })?;
            let itunes_item_extension = ITunesItemExtensionBuilder::default()
                .summary(Some(description))
                .duration(Some({
                    let hours = video_infos.duration.hour;
                    let minutes = video_infos.duration.minute;
                    let seconds = video_infos.duration.second;
                    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
                }))
                .image(get_thumb_from_playlist_item_snippet(snippet).and_then(|t| t.url))
                .build();
            item_builder.itunes_ext(Some(itunes_item_extension));
            Some(item_builder.build())
        })
        .collect();
    rss_item
}

async fn fetch_playlist_items(
    playlist_id: &String,
    api_key: &String,
) -> eyre::Result<Vec<PlaylistItem>> {
    let hub = get_youtube_hub();
    let max_consecutive_requests = 5;
    let mut fetched_playlist_items: Vec<PlaylistItem> =
        Vec::with_capacity(max_consecutive_requests * 50);
    let mut request_count = 0;
    let mut next_page_token: Option<String> = None;
    debug!("fetching items from playlist {}", playlist_id);
    loop {
        let mut playlist_items_request = hub
            .playlist_items()
            .list(&vec!["snippet".into()])
            .playlist_id(playlist_id)
            .param("key", &api_key)
            .max_results(50);
        if let Some(ref next_page_token) = next_page_token {
            playlist_items_request = playlist_items_request.page_token(next_page_token.as_str());
        }
        let response = playlist_items_request.doit().await?;

        fetched_playlist_items.extend(response.1.items.ok_or(eyre!("playlist has no items"))?);
        next_page_token = response.1.next_page_token;

        request_count += 1;
        if next_page_token.is_none() || request_count > max_consecutive_requests {
            info!(
                "fetched {} items, channel too large, stopping",
                fetched_playlist_items.len()
            );
            break;
        }
    }
    info!(
        "fetched {} items, in {} requests",
        fetched_playlist_items.len(),
        request_count
    );
    fetched_playlist_items.sort_by_key(|i| i.snippet.as_ref().and_then(|s| s.published_at));
    Ok(fetched_playlist_items)
}

fn build_channel_from_playlist(playlist: api::Playlist) -> Channel {
    let mut channel_builder = ChannelBuilder::default();
    let mut itunes_channel_builder = ITunesChannelExtensionBuilder::default();

    if let Some(mut snippet) = playlist.snippet {
        channel_builder.description(snippet.description.take().unwrap_or("".to_owned()));
        channel_builder.title(snippet.title.take().unwrap_or("".to_owned()));
        channel_builder.language(snippet.default_language.take());
        channel_builder.link(format!(
            "https://www.youtube.com/playlist?list={}",
            playlist.id.unwrap_or_default()
        ));
        if let Some(mut thumb) = get_thumb_from_playlist_snippet(snippet) {
            itunes_channel_builder.image(thumb.url.take());
        }
    }

    channel_builder.itunes_ext(Some(itunes_channel_builder.build()));
    channel_builder.build()
}

async fn fetch_playlist(id: String, api_key: &String) -> Result<api::Playlist, eyre::Error> {
    let hub = get_youtube_hub();
    let playlist_request = hub
        .playlists()
        .list(&vec!["snippet".into()])
        .add_id(&id)
        .param("key", &api_key);
    let result = playlist_request.doit().await?;
    let playlist = result
        .1
        .items
        .ok_or(eyre!("youtube returned no playlist with id {:?}", id))?
        .first()
        .ok_or(eyre!("youtube returned no playlist with id {:?}", id))?
        .clone();
    Ok(playlist)
}

fn get_youtube_hub() -> YouTube<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>> {
    let auth = google_youtube3::client::NoToken;
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http1()
        .build();
    let client = hyper::Client::builder().build(connector);
    let hub = YouTube::new(client, auth);
    hub
}

#[io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, Url>",
    create = r##" {
        AsyncRedisCache::new("cached_yt_stream_url=", 18000)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
            .build()
            .await
            .expect("get_youtube_stream_url cache")
} "##
)]
async fn get_youtube_stream_url(url: &Url) -> eyre::Result<Url> {
    debug!("getting stream_url for yt video: {}", url);
    let output = tokio::process::Command::new("yt-dlp")
        .arg("-f")
        .arg("bestaudio")
        .arg("--get-url")
        .arg(url.as_str())
        .output()
        .await;

    match output {
        Ok(x) => {
            let raw_url = std::str::from_utf8(&x.stdout).unwrap_or_default();
            match Url::from_str(raw_url) {
                Ok(url) => Ok(url),
                Err(e) => {
                    warn!(
                        "error while parsing stream url:\ninput: {}\nerror: {}",
                        raw_url,
                        e.to_string()
                    );
                    Err(eyre::eyre!(e))
                }
            }
        }
        Err(e) => Err(eyre::eyre!(e)),
    }
}

async fn feed_url_for_yt_playlist(url: &Url) -> eyre::Result<Url> {
    let playlist_id = url
        .query_pairs()
        .find(|(key, _)| key == "list")
        .map(|(_, value)| value)
        .ok_or_else(|| eyre::eyre!("Failed to parse playlist ID from URL: {}", url))?;

    let mut feed_url = Url::parse("https://www.youtube.com/feeds/videos.xml").unwrap();
    feed_url
        .query_pairs_mut()
        .append_pair("playlist_id", &playlist_id);

    Ok(feed_url)
}
async fn feed_url_for_yt_atom(url: &Url) -> eyre::Result<Url> {
    Ok(url.clone())
}

async fn feed_url_for_yt_channel(url: &Url) -> eyre::Result<Url> {
    info!("trying to convert youtube channel url {}", url);
    if url.to_string().contains("feeds/videos.xml") {
        return Ok(url.to_owned());
    }
    let url_with_channel_id = find_yt_channel_url_with_c_id(url).await?;
    let channel_id = url_with_channel_id.path_segments().unwrap().last().unwrap();
    let mut feed_url = Url::parse("https://www.youtube.com/feeds/videos.xml")?;
    feed_url
        .query_pairs_mut()
        .append_pair("channel_id", channel_id);
    info!("converted to {feed_url}");
    Ok(feed_url)
}

#[cfg_attr(
    not(test),
    io_cached(
        map_error = r##"|e| eyre::Error::new(e)"##,
        type = "AsyncRedisCache<Url, Url>",
        create = r##" {
        AsyncRedisCache::new("youtube_channel_username_to_id=", 9999999)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
            .build()
            .await
            .expect("youtube_channel_username_to_id cache")
} "##
    )
)]
async fn find_yt_channel_url_with_c_id(url: &Url) -> eyre::Result<Url> {
    info!("conversion not in cache, using yt-dlp for conversion...");
    let output = Command::new("yt-dlp")
        .arg("--playlist-items")
        .arg("0")
        .arg("-O")
        .arg("playlist:channel_url")
        .arg(url.to_string())
        .output()
        .await?;
    let feed_url = std::str::from_utf8(&output.stdout)?.trim().to_string();
    Ok(Url::parse(&feed_url)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use test_log::test;

    #[tokio::test]
    async fn test_build_items_for_playlist() {
        let id = "PLJmimp-uZX42T7ONp1FLXQDJrRxZ-_1Ct".to_string();
        let api_key = conf().get(ConfName::YoutubeApiKey).unwrap();

        let playlist = fetch_playlist(id, &api_key).await.unwrap();

        println!("{:?}", &playlist.clone().id.unwrap().clone());
        let items = fetch_playlist_items(&playlist.id.unwrap(), &api_key)
            .await
            .unwrap();

        println!("{:?}", items);
        assert!(items.len() > 0)
    }

    #[test(tokio::test)]
    async fn test_build_channel_for_playlist() {
        let id = "PLJmimp-uZX42T7ONp1FLXQDJrRxZ-_1Ct".to_string();
        let api_key = conf().get(ConfName::YoutubeApiKey).unwrap();

        let playlist = fetch_playlist(id, &api_key).await.unwrap();

        let channel = build_channel_from_playlist(playlist);

        println!("{:?}", channel);
        assert!(channel.description.len() > 0);
        assert!(channel.title.len() > 0);
        assert!(channel.itunes_ext.unwrap().image.is_some());
    }

    #[test(tokio::test)]
    async fn test_fetch_playlist() {
        let id = "PLJmimp-uZX42T7ONp1FLXQDJrRxZ-_1Ct".to_string();
        let api_key = conf().get(ConfName::YoutubeApiKey).unwrap();

        let result = fetch_playlist(id, &api_key).await;

        println!("{:?}", result);
        assert!(result.is_ok());

        if let Ok(playlist) = result {
            assert!(playlist.id.is_some());
            assert!(playlist.snippet.is_some());
        }
    }

    #[test(tokio::test)]
    async fn test_fetch_youtube_channel_by_name() {
        let provider = YoutubeProvider::new();
        let Ok(_api_key) = conf().get(ConfName::YoutubeApiKey) else {
            panic!("to run this test you need to set an api key for youtube.");
        };

        let result = provider
            .generate_rss_feed(Url::parse("https://www.youtube.com/@LegalEagle").unwrap())
            .await;

        assert!(result.is_ok());
        let feed = feed_rs::parser::parse(&result.unwrap().into_bytes()[..]).unwrap();

        assert!(feed.entries.len() > 50);
        for entry in &feed.entries {
            for media in &entry.media {
                let duration = media.duration.unwrap();
                assert!(duration > Duration::default());
            }
            assert!(entry.title.is_some());
            assert!(entry.summary.is_some());
        }
    }
}

