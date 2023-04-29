use async_trait::async_trait;
use log::{info, error};
use reqwest::Url;
use tokio::process::Command;
#[allow(unused_imports)]
use cached::AsyncRedisCache;
#[allow(unused_imports)]
use cached::proc_macro::io_cached;

use crate::configs::{conf, ConfName, Conf};

pub fn check_if_in_whitelist(url: &Url) -> bool {
    let valid_domains: Vec<Url> = conf().get(ConfName::ValidUrlDomains).unwrap().split(",").map(|s| Url::parse(s).unwrap()).collect();
    let is_valid = valid_domains.iter().any(|valid_domain| {
        url.scheme() == valid_domain.scheme() && url.host_str() == valid_domain.host_str()
    });
    return is_valid;
}

pub fn from(url: Url) -> Box<dyn ConvertableToFeed> {
    let url = url.to_owned();
    match url {
        _ if url.domain().unwrap_or_default().contains("twitch.tv") => Box::new(TwitchUrl { url }),
        _ if url.domain().unwrap_or_default().contains("youtube.com") || url.domain().unwrap_or_default().contains("youtu.be") => {
            match url {
                _ if url.path().starts_with("/playlist") => Box::new(YoutubePlaylistUrl { url }),
                _ if url.path().starts_with("/channel") => Box::new(YoutubeChannelUrl { url }),
                _ if url.path().starts_with("/user") => Box::new(YoutubeChannelUrl { url }),
                _ if url.path().starts_with("/c") => Box::new(YoutubeChannelUrl { url }),
                _ if url.path().starts_with("/") => Box::new(YoutubeChannelUrl { url }),
                _ => Box::new(UnsupportedYoutubeUrl { url }),
            }
        },
        _ => Box::new(GenericUrl { url }),
    }
}

#[async_trait]
pub trait ConvertableToFeed {
    async fn to_feed_url(&self) -> eyre::Result<Url>;
}

struct TwitchUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for TwitchUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        info!("trying to convert twitch channel url {}", self.url);
        let username = self.url.path_segments().ok_or_else(|| eyre::eyre!("Unable to get path segments"))?.last().ok_or_else(|| eyre::eyre!("Unable to get last path segment"))?;
        let mut ttprss_url = conf().get(ConfName::TwitchToPodcastUrl)?;
        if !ttprss_url.starts_with("http://") && !ttprss_url.starts_with("https://") {
            ttprss_url = format!("http://{}", ttprss_url);
        }
        let mut feed_url = Url::parse(&format!("{}{}", ttprss_url, "/vod")).map_err(|err| eyre::eyre!(err))?;
        feed_url.path_segments_mut().map_err(|_| eyre::eyre!("Unable to get mutable path segments"))?.push(username);
        feed_url.query_pairs_mut().append_pair("transcode", "false");
        info!("converted to {feed_url}");
        Ok(feed_url)
    }
}

struct YoutubePlaylistUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for YoutubePlaylistUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        let playlist_id = self.url.query_pairs()
            .find(|(key, _)| key == "list")
            .map(|(_, value)| value)
            .ok_or_else(|| eyre::eyre!("Failed to parse playlist ID from URL: {}", self.url))?;

        let podtube_api_key = conf().get(ConfName::YoutubeApiKey).ok().and_then(|s| if s.is_empty() { None } else { Some(s) });
        if let (Ok(mut podtube_url), Some(_)) = (Url::parse(&conf().get(ConfName::PodTubeUrl).unwrap_or_default()), podtube_api_key) {
            podtube_url.set_path(format!("/youtube/playlist/{playlist_id}").as_str());
            Ok(podtube_url)
        } else {
            let mut feed_url = Url::parse("https://www.youtube.com/feeds/videos.xml").unwrap();
            feed_url.query_pairs_mut().append_pair("playlist_id", &playlist_id);

            Ok(feed_url)
        }

    }
}

struct YoutubeChannelUrl {
    url: Url,
}

#[cfg_attr(not(test),
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
))]
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

#[async_trait]
impl ConvertableToFeed for YoutubeChannelUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        info!("trying to convert youtube channel url {}", self.url);
        if self.url.to_string().contains("feeds/videos.xml") {
            return Ok(self.url.to_owned());
        }
        let url_with_channel_id = find_yt_channel_url_with_c_id(&self.url).await?;
        let channel_id = url_with_channel_id.path_segments().unwrap().last().unwrap();
        let podtube_api_key = conf().get(ConfName::YoutubeApiKey).ok();
        if let (Ok(mut podtube_url), Some(_)) = (Url::parse(&conf().get(ConfName::PodTubeUrl).unwrap_or_default()), podtube_api_key) {
            podtube_url.set_path(format!("/youtube/channel/{channel_id}").as_str());
            Ok(podtube_url)
        } else {
            let mut feed_url = Url::parse("https://www.youtube.com/feeds/videos.xml")?;
            feed_url.query_pairs_mut().append_pair("channel_id", channel_id);
            info!("converted to {feed_url}");
            Ok(feed_url)
        }
    }
}

struct UnsupportedYoutubeUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for UnsupportedYoutubeUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        error!("unsupported yotube url found: {}", &self.url);
        Err(eyre::eyre!("unsupported yotube url found: {}", &self.url))
    }
}

struct GenericUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for GenericUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        info!("generic feed detected no need for conversion");
        Ok(self.url.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_twitch_to_feed() {
        std::env::set_var("TWITCH_TO_PODCAST_URL", "ttprss");
        let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
        println!("{:?}", url);
        let feed_url = from(url).to_feed_url().await.unwrap();
        println!("{:?}", feed_url);
        assert_eq!(feed_url.as_str(), "http://ttprss/vod/andersonjph?transcode=false");
    }

    #[tokio::test]
    async fn test_http_ttprss_twitch_to_feed() {
        std::env::set_var("TWITCH_TO_PODCAST_URL", "http://ttprss");
        let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
        println!("{:?}", url);
        let feed_url = from(url).to_feed_url().await.unwrap();
        println!("{:?}", feed_url);
        assert_eq!(feed_url.as_str(), "http://ttprss/vod/andersonjph?transcode=false");
    }

    #[tokio::test]
    async fn test_ssl_ttprss_twitch_to_feed() {
        std::env::set_var("TWITCH_TO_PODCAST_URL", "https://ttprss");
        let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
        println!("{:?}", url);
        let feed_url = from(url).to_feed_url().await.unwrap();
        println!("{:?}", feed_url);
        assert_eq!(feed_url.as_str(), "https://ttprss/vod/andersonjph?transcode=false");
    }


    #[tokio::test]
    async fn test_youtube_to_feed() {

        temp_env::async_with_vars([
            ("YT_API_KEY", Some("")),
            ("PODTUBE_URL", None)
        ], test()).await;

        async fn test() {
            let url = "https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw";
            println!("{:?}", url);
            let channel = YoutubeChannelUrl { url: Url::parse(url).unwrap() };
            let feed_url = channel.to_feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw");
        }
    }

    #[tokio::test]
    async fn test_youtube_playlist_to_feed() {
        temp_env::async_with_vars([
            ("YT_API_KEY", Some("")),
            ("PODTUBE_URL", None)
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.youtube.com/playlist?list=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD").unwrap();
            println!("{:?}", url);
            let feed_url = from(url).to_feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?playlist_id=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD");
        }
    }

    #[tokio::test]
    async fn test_youtube_to_feed_already_atom_feed() {
        let url = Url::parse("https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
        println!("{:?}", url);
        let feed_url = from(url).to_feed_url().await.unwrap();
        println!("{:?}", feed_url);
        assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw");
    }

    #[tokio::test]
    async fn test_generic_to_feed() {
        let url = Url::parse("https://feeds.simplecast.com/aU_RzZ7j").unwrap();
        println!("{:?}", url);
        let feed_url = from(url).to_feed_url().await.unwrap();
        println!("{:?}", feed_url);
        assert_eq!(feed_url.as_str(), "https://feeds.simplecast.com/aU_RzZ7j");
    }

    #[tokio::test]
    async fn test_youtube_to_feed_api_key_present() {

        temp_env::async_with_vars([
            ("YT_API_KEY", Some("1234567890123456780")),
            ("PODTUBE_URL", Some("http://podtube"))
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
            println!("{:?}", url);
            let feed_url = from(url).to_feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(feed_url.as_str(), "http://podtube/youtube/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw");
        }
    }

    #[tokio::test]
    async fn test_youtube_playlist_to_feed_api_key_present() {

        temp_env::async_with_vars([
            ("YT_API_KEY", Some("1234567890123456780")),
            ("PODTUBE_URL", Some("http://podtube"))
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.youtube.com/playlist?list=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD").unwrap();
            println!("{:?}", url);
            let feed_url = from(url).to_feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(feed_url.as_str(), "http://podtube/youtube/playlist/PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD");
        }
    }
}
