//this module will take an existing RSS and output a new RSS with the enclosure replaced by the trascode URL
//usage example
//GET /rss?url=https://website.com/url/to/feed.rss
//media will have original url replaced by
//GET /transcode/UUID?url=https://website.com/url/to/media.mp3
//GET /transcode/UUID?url=https://website.com/url/to/media.mp4
//GET /transcode/UUID?url=https://website.com/url/to/media.m3u8 (this will require some trickery to get the correct duration)
//in the frist version those calls will use the duration field in the RSS with the constant bitrate of the transcoder to generate
//a correct byte range
//in future version they will try to get the first secs of the original to get the bitrate and original byte range

use std::fmt::Display;
use std::time::Duration;

use cached::AsyncRedisCache;
use cached::proc_macro::io_cached;
use eyre::eyre;
use feed_rs::model::Entry;
use log::{warn, debug};
use reqwest::Url;
use rss::{GuidBuilder, Guid};
use rss::{ Enclosure, ItemBuilder, Item, extension::itunes::ITunesItemExtensionBuilder };
use feed_rs::parser;
use tokio::process::Command;

use std::str;

#[io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<String, u64>",
    create = r##" {
    AsyncRedisCache::new("cached_yt_video_duration=", 9999999)
        .set_refresh(true)
        .set_connection_string("redis://redis:6379/")
        .build()
        .await
        .expect("youtube_duration cache")
} "##
)]
async fn get_youtube_video_duration(url: String) -> eyre::Result<u64> {
    debug!("getting duration for yt video: {}", url);
    let output = Command::new("yt-dlp")
        .arg("--get-duration")
        .arg(url)
        .output()
    .await;

    if let Ok(x) = output {
        let duration_str = str::from_utf8(&x.stdout).unwrap().trim().to_string();
        Ok(parse_duration(&duration_str).unwrap_or_default().as_secs())
    } else {
        warn!("could not parse youtube video duration");
        Ok(0)
    }

}

fn parse_duration(duration_str: &str) -> Result<Duration, String> {
    let duration_parts: Vec<&str> = duration_str.split(':').rev().collect();

    let seconds = if let Some(sec_str) = duration_parts.get(0) {
        sec_str.parse().map_err(|_| "Invalid format".to_string())?
    } else { 0 };

    let minutes = if let Some(min_str) = duration_parts.get(1) {
        min_str.parse().map_err(|_| "Invalid format".to_string())?
    } else { 0 };

    let hours= if let Some(hour_str) = duration_parts.get(2) {
        hour_str.parse().map_err(|_| "Invalid format".to_string())?
    } else { 0 };

    let duration_secs= hours * 3600 + minutes * 60 + seconds;
    Ok(Duration::from_secs(duration_secs))
}

pub struct RssTranscodizer {
    url: String,
    transcode_service_url: Url,
}


impl RssTranscodizer {
    pub fn new(url: String, transcode_service_url: Url) -> Self {
        Self { url, transcode_service_url }
    }


    pub async fn transcodize(&self) -> eyre::Result<String> {

        cached_transcodize(TranscodeParams {
            transcode_service_url_str: self.transcode_service_url.clone().to_string(),
            url: self.url.clone()
        }).await
    }
}

#[derive(Clone, Hash)]
struct TranscodeParams {
    transcode_service_url_str: String,
    url: String
}

impl Display for TranscodeParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", &self.transcode_service_url_str, &self.url)
    }
}

#[io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<TranscodeParams, String>",
    create = r##" {
        AsyncRedisCache::new("cached_transcodizer=", 600)
            .set_refresh(true)
            .set_connection_string("redis://redis:6379/")
            .build()
            .await
            .expect("rss_transcodizer cache")
    } "##
)]
async fn cached_transcodize(input: TranscodeParams) -> eyre::Result<String> {
    let transcode_service_url = Url::parse(&input.transcode_service_url_str).unwrap();
    let rss_body = (async { reqwest::get(&input.url).await?.bytes().await }).await?;
    let feed = match parser::parse(&rss_body[..]) {
        Ok(x) => {
            debug!("parsed feed");
            x
        },
        Err(e) => {
            return Err(eyre!("could not parse rss, reason: {e}"));
        }
    };

    let mut feed_builder = rss::ChannelBuilder::default();
    if let Some(x) = feed.title {
        debug!("Title found: {}", x.content);
        feed_builder.title(x.content);
    }

    if let Some(x) = feed.description {
        debug!("Description found: {}", x.content);
        feed_builder.description(x.content);
    }

    if let Some(x) = feed.logo {
        debug!("image found, logo branch: x={:?}", x);
        let mut img_buider = rss::ImageBuilder::default();
        img_buider.url(x.uri);
        img_buider.link(x.title.unwrap_or_default());
        img_buider.description(Some(x.description.unwrap_or_default()));
        feed_builder.image(Some(img_buider.build()));
    } else {
        if let Some(x) = feed.icon {
            debug!("image found, icon branch: x={:?}", x);
            let mut img_buider = rss::ImageBuilder::default();
            img_buider.url(x.uri);
            img_buider.link(x.title.unwrap_or_default());
            img_buider.description(Some(x.description.unwrap_or_default()));
            feed_builder.image(Some(img_buider.build()));
        }
    }

    feed_builder.generator(Some("generated by VodToPodcastRss".to_string()));

    struct ConvertItemsParams {
        item: Entry,
        transcode_service_url: Url
    }

    async fn convert_item(params: ConvertItemsParams) -> Option<Item> {
        let item = params.item;
        let transcode_service_url = params.transcode_service_url;

        let mut item_builder = ItemBuilder::default();
        let mut itunes_builder = ITunesItemExtensionBuilder::default();

        if let Some(x) = &item.title {
            debug!("item title: {}", x.content);
            item_builder.title(Some(x.content.clone()));
        }

        let found_url: Option<Url> = { //FIX: when I stop sucking at rust
            debug!("searching for stream url: {:?}", item.media);
            let mut media_links = Vec::new();
            for x in item.media.iter() {
                let mut found_url: Option<Url> = None;
                for y in x.content.iter() {
                    let content_t = y.content_type.clone();
                    match content_t.unwrap().to_string() == "audio/mpeg".to_string() {
                        true => {
                            found_url = y.url.clone();
                            break;
                        }
                        false => (),
                    }
                }
                if let Some(url) = found_url {
                    media_links.push(url);
                }
            }
            let first_link = media_links.first();
            let url: Option<Url> = match first_link {
                Some(x) => Some(x.clone()),
                _ => None,
            };
            debug!("stream url found: {:?}", url);
            url
        };

        let media_url: Url = match found_url {
            Some(x) => {
                debug!("media url found: {:?}", x);
                x
            },
            _ => {
                let url_link = item.links.iter().find(|x| {
                    debug!("pondering on link {:?}", x);
                    let regex = regex::Regex::new("^(https?://)?(www\\.youtube\\.com|youtu\\.be)/.+$").unwrap();
                    regex.is_match(x.href.as_str())
                });
                if let Some(url) = url_link {
                    debug!("media url found inside links: {:?}", url_link);
                    Url::parse(url.href.to_string().as_str()).unwrap()
                } else {
                    return Some(item_builder.build());
                }
            }
        };

        let media_element = match item.media.first() {
            Some(x) => x,
            None => {
                warn!("no media element found");
                return None
            }
        };

        if let Some(x) = &media_element.description {
            debug!("Description found: {:?}", x.content);
            item_builder.description(Some(x.content.clone()));
        }
        else if let Some(x) = &item.content {
            debug!("Description found: {:?}", x);
            item_builder.description(Some(x.body.clone().unwrap_or_default()));
        }
        else if let Some(x) = &item.summary {
            debug!("Description found: {:?}", x);
            item_builder.description(Some(x.content.to_string().clone()));
        };

        if let Some(x) = item.published {
            debug!("Published date found: {:?}", x.to_rfc2822());
            item_builder.pub_date(Some(x.to_rfc2822()));
        }

        if let Some(x) = &media_element.thumbnails.first() {
            debug!("Thumbnail image found: {:?}", x.image.uri);
            itunes_builder.image(Some(x.image.uri.clone()));
        }

        let duration = match media_element.duration {
            Some(x) => x,
            None => {
                debug!("runnign regex on url: {}", media_url.as_str());
                let youtube_regex = regex::Regex::new(r#"^(https?://)?(www\.)?(youtu\.be/|youtube\.com/)"#).unwrap();
                if youtube_regex.is_match(media_url.as_str()) {
                    let duration = get_youtube_video_duration(media_url.as_str().to_string()).await.unwrap();
                    debug!("duration of {duration} extracted");
                    Duration::from_secs(duration)
                } else {
                    warn!("no duration found");
                    Duration::default()
                }
            }
        };

        let duration_secs = duration.as_secs();
        let duration_string = format!("{:02}:{:02}:{:02}", duration_secs / 3600, (duration_secs % 3600) / 60, (duration_secs % 60));
        itunes_builder.duration(Some(duration_string));
        let mut transcode_service_url = transcode_service_url;
        let bitrate = 64; //TODO: refactor
        let generation_uuid  = uuid::Uuid::new_v4().to_string();
        transcode_service_url
            .query_pairs_mut()
            .append_pair("url", media_url.as_str())
            .append_pair("bitrate", bitrate.to_string().as_str())
            .append_pair("uuid", generation_uuid.as_str())
            .append_pair("duration", duration_secs.to_string().as_str());

        let enclosure = Enclosure {
            length: (bitrate * 1024 * duration_secs).to_string(),
            url: transcode_service_url.to_string(),
            mime_type: "audio/mpeg".to_string(),
        };

        debug!("setting enclosure to: \nlength: {}, url: {}, mime_type: {}", enclosure.length, enclosure.url, enclosure.mime_type);

        let guid = generate_guid(&item.id);
        item_builder.guid(Some(guid));
        item_builder.enclosure(Some(enclosure));

        item_builder.itunes_ext(Some(itunes_builder.build()));
        debug!("item parsing completed!\n------------------------------\n------------------------------");
        Some(item_builder.build())
    }

    use futures::stream::FuturesUnordered;
    use futures::StreamExt;

    let futures = FuturesUnordered::new();
    for entry in feed.entries.iter() {
        let ser_url = transcode_service_url.clone();
        futures.push(async move { convert_item(ConvertItemsParams { item: entry.to_owned(), transcode_service_url: ser_url}).await });
    }

    let item_futures: Vec<_> =  futures.collect().await;
    let feed_items: Vec<_> = item_futures.iter().filter_map(|x| if !x.is_none() { Some(x.to_owned().unwrap())} else { None }).collect();

    feed_builder.items(feed_items);
    Ok(feed_builder.build().to_string())
}

fn generate_guid(url: &str) -> Guid {
    let digest: md5::Digest = md5::compute(url);
    let md5_bytes_from_digest = digest.0;
    let uuid_for_item: uuid::Builder = uuid::Builder::from_md5_bytes(md5_bytes_from_digest);
    let uuid = uuid_for_item.as_uuid().clone();
    let mut guid_builder = GuidBuilder::default();
    guid_builder.permalink(true).value(uuid.to_string());
    guid_builder.build()
}


#[cfg(test)]
mod test {

    use std::sync::Mutex;

    use actix_web::{HttpServer, App, web::{self, Data}, HttpResponse, http::header::ContentType, rt, dev::ServerHandle};
    use rss::Channel;

    use super::*;

    async fn validate_must_have_props(rss_transcodizer: RssTranscodizer) -> Result<(), String> {
        println!("fake server up");
        println!("transcodizing");

        let transcodized_rss = rss_transcodizer
            .transcodize()
            .await
            .expect("failed to transcodize RSS feed");

        println!("parsing feed");
        // Parse the transcodized RSS to make it easier to compare
        let transcodized_rss_parsed = Channel::read_from(transcodized_rss.as_bytes()).unwrap();
        println!("checking feed validity");

        _ = match transcodized_rss_parsed.generator() {
            None => return Err(format!("missing generator field")),
            _ => (),
        };

        if transcodized_rss_parsed.items().len() == 0 {
            return Err("no items found".to_string());
        }

        for i in transcodized_rss_parsed.items().iter() {
            let pretty_item = serde_json::to_string(&i).unwrap();

            if i.itunes_ext().is_none() {
                return Err(format!("missing itunes extensions {:?}", pretty_item));
            }

            if i.itunes_ext().unwrap().duration().is_none() {
                return Err(format!("missing duration {:?}", pretty_item));
            }

            if i.guid().is_none() {
                return Err(format!("missing guid for item {:?}", pretty_item));
            }

            if i.enclosure.is_none() {
                return Err(format!("missing enclosure for item {:?}", pretty_item));
            }

            if i.title.is_none() {
                return Err(format!("missing title for item {:?}", pretty_item));
            }

            if i.description.is_none() {
                return Err(format!("missing description for item {:?}", pretty_item));
            }
        }
        return Ok(())
    }

    async fn stop_test(handle: ServerHandle) {
        handle.stop(false).await
    }

    #[actix_web::test]
    async fn rss_podcast_feed() -> Result<(), String> {
        let handle = startup_test("podcast".to_string() , 9872).await;

        let rss_url = "http://127.0.0.1:9872/feed.rss".to_string();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9872/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url);

        let res = validate_must_have_props(rss_transcodizer).await;

        stop_test(handle).await;

        res
    }

    #[actix_web::test]
    async fn rss_twitch_feed() -> Result<(), String> {
        let handle = startup_test("twitch".to_string(), 9871).await;

        let rss_url = "http://127.0.0.1:9871/feed.rss".to_string();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9871/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url);


        let res = validate_must_have_props(rss_transcodizer).await;

        stop_test(handle).await;

        res
    }

    #[actix_web::test]
    async fn rss_youtube_feed() -> Result<(), String> {
        let handle = startup_test("youtube".to_string(), 9870).await;

        let rss_url = "http://127.0.0.1:9870/feed.rss".to_string();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9870/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url);

        let res = validate_must_have_props(rss_transcodizer).await;

        stop_test(handle).await;

        res
    }

    async fn startup_test(type_test: String, port: u16) -> ServerHandle {
        let feed_type = Data::new(type_test);
        let fake_server = HttpServer::new(move || {
            App::new()
                .app_data(Data::clone(&feed_type))
                .route(
                    "/feed.rss",
                    web::get().to(|feed_type: Data<String>| async move {
                        let url = format!("./src/rss_transcodizer/sample_rss/{}.rss", feed_type.to_string());
                        println!("reading rss from file: {url}");
                        let raw_rss_str = std::fs::read_to_string(url).unwrap();
                        HttpResponse::Ok()
                            .content_type(ContentType::xml())
                            .body(raw_rss_str)
                    }))
                .route("/ready", web::get().to(|| async move {HttpResponse::Ok()}))
        }).bind(("127.0.0.1", port)).unwrap().run();
        let handle = fake_server.handle();
        rt::spawn(fake_server);
        handle
    }
}
