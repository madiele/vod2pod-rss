use std::fmt::Display;
use std::time::Duration;

#[allow(unused_imports)]
use cached::AsyncRedisCache;
#[allow(unused_imports)]
use cached::proc_macro::io_cached;
use chrono::DateTime;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use eyre::eyre;
use feed_rs::model::{Entry, MediaObject};
use log::{warn, debug, error};
use reqwest::Url;
use rss::{GuidBuilder, Guid};
use rss::{ Enclosure, ItemBuilder, Item, extension::itunes::ITunesItemExtensionBuilder };
use feed_rs::parser;
use tokio::process::Command;

use std::str;

use crate::configs::{conf, ConfName, Conf};

#[cfg_attr(not(test),
io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, u64>",
    create = r##" {
        AsyncRedisCache::new("cached_yt_video_duration=", 86400)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
            .build()
            .await
            .expect("youtube_duration cache")
} "##
))]
async fn get_youtube_video_duration(url: &Url) -> eyre::Result<u64> {
    debug!("getting duration for yt video: {}", url);

    if let Ok(_) = conf().get(ConfName::YoutubeApiKey) {
        let id = url.query_pairs().find(|(key, _)| key == "v").map(|(_, value)| {value}).unwrap().to_string();
        Ok(get_yotube_duration_with_apikey(&id).await?.as_secs())
    } else {
        acquire_semaphore("yt_duration_semaphore".to_string(), url.to_string()).await?;
        let output = Command::new("yt-dlp")
            .arg("--get-duration")
            .arg(url.to_string())
            .output()
        .await;
        release_semaphore("yt_duration_semaphore".to_string(), url.to_string()).await?;
        if let Ok(x) = output {
            let duration_str = str::from_utf8(&x.stdout).unwrap().trim().to_string();
            Ok(parse_duration(&duration_str).unwrap_or_default().as_secs())
        } else {
            warn!("could not parse youtube video duration");
            Ok(0)
        }
    }


}

//TODO: refactor in to cache abstraction a module
async fn acquire_semaphore(semaphore_name: String, id: String) -> eyre::Result<()> {
    let redis_url = conf().get(ConfName::RedisUrl)?;
    let client = redis::Client::open(redis_url)?;
    let mut con = client.get_tokio_connection().await?;
    let unix_timestamp_now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let mut pipe = redis::pipe();

    pipe.zrembyscore(&semaphore_name, "-inf", unix_timestamp_now - 600);

    pipe.zadd(&semaphore_name, &id, unix_timestamp_now);

    pipe.query_async(&mut con).await?;

    loop {
        let count: u64 = redis::cmd("ZRANK").arg(&semaphore_name).arg(&id).query_async(&mut con).await?;
        if count <= 4 { debug!("semaphore {count} <= 4 letting {id} in"); break; }
        actix_rt::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}

async fn release_semaphore(semaphore_name: String, id: String) -> eyre::Result<()> {
    let redis_url = conf().get(ConfName::RedisUrl)?;
    let client = redis::Client::open(redis_url)?;
    let mut con = client.get_tokio_connection().await?;

    debug!("releasing semaphore for {id}");
    _ = redis::cmd("ZREM").arg(semaphore_name).arg(id).query_async(&mut con).await?;

    Ok(())
}

//I know this code is over-engineered to hell, please don't judge me. I just wanted play around with redis
async fn get_yotube_duration_with_apikey(id: &String) -> eyre::Result<Duration> {
    let redis_url = conf().get(ConfName::RedisUrl)?;
    let client = redis::Client::open(redis_url)?;
    let mut con = client.get_tokio_connection().await?;
    let lock_key = "youtube_duration_lock";
    let queue_key = "youtube_duration_queue";
    let hash_key = "youtube_duration_batch";
    let lock_duration_secs = 10;
    let batch_size = 50;
    let sleep_duration = 300;

    let _: i64 = redis::cmd("RPUSH").arg(queue_key).arg(&id)
        .query_async(&mut con).await?;
    debug!("Inserted video with ID {} into Redis queue", &id);

    let lock: bool = redis::cmd("SET").arg(lock_key).arg("1").arg("NX").arg("EX").arg(lock_duration_secs)
        .query_async(&mut con).await?;

    if lock {
        debug!("Redis lock has been acquired successfully");
        actix_rt::time::sleep(std::time::Duration::from_millis(sleep_duration)).await;

        while let Ok(queue_len)  = redis::cmd("LLEN").arg(queue_key).query_async::<_,i64>(&mut con).await {
            if queue_len == 0 {break};
            let batch_end = queue_len - 1;
            let batch_start = batch_end - batch_size + 1;
            let batch: Vec<String> = redis::cmd("LRANGE").arg(queue_key).arg(batch_start).arg(batch_end)
                .query_async(&mut con).await?;
            debug!("Processing batch of {} items from Redis queue", batch.len());

            let api_key = conf().get(ConfName::YoutubeApiKey)?;
            let mut url = Url::parse("https://www.googleapis.com/youtube/v3/videos")?;
            url.query_pairs_mut()
                .append_pair("id", &batch.join(","))
                .append_pair("part", "contentDetails")
                .append_pair("key", &api_key);

            let response = reqwest::get(url).await?.json::<serde_json::Value>().await?;
            debug!("retrived video infos from youtube apis");

            let items = response["items"].as_array().unwrap();
            let mut durations_by_id = Vec::new();
            for item in items {
                let id = item["id"].as_str().ok_or_else(|| eyre!("Failed to retrieve ID from item"))?.to_owned();
                let duration_str = item["contentDetails"]["duration"].as_str().ok_or_else(|| eyre!("Failed to retrieve duration string from item."))?;
                let duration = iso8601_duration::Duration::parse(duration_str).map_err(|_| eyre!("could not parse yotube duration"))?;
                durations_by_id.push((id, duration));
            }

            let mut pipe = redis::pipe();
            for (id, duration) in durations_by_id {
                pipe.hset(hash_key, id, duration.num_seconds().unwrap_or(0.0).round() as i32).ignore();
            }
            pipe.ltrim(queue_key, 0, (batch_start - 1).try_into()?).ignore();
            pipe.del(lock_key).ignore();
            let _: () = pipe.query_async(&mut con).await?;
            debug!("finished retriving batch of durations with youtube api");
        }
    }

    let mut duration_secs_str: Option<String> = None;
    let mut count = 0;
    while duration_secs_str.is_none() && count < 100 {
        duration_secs_str = redis::cmd("HGET").arg(hash_key).arg(id).query_async(&mut con).await.ok();
        if duration_secs_str.is_none() {actix_rt::time::sleep(Duration::from_millis(sleep_duration / 2)).await};
        count += 1;
    }
    debug!("duration found for {id}: {}", duration_secs_str.clone().unwrap_or("???".to_string()));
    let duration = Duration::from_secs(duration_secs_str.ok_or_else(|| eyre!("duration_secs_str is None"))?.parse()?);
    return Ok(duration);
}

fn parse_duration(duration_str: &str) -> Result<Duration, String> {
    let duration_parts: Vec<&str> = duration_str.split(':').rev().collect();

    let seconds = match duration_parts.get(0) {
        Some(sec_str) => sec_str.parse().map_err(|_| "Invalid format".to_string())?,
        None => 0,
    };

    let minutes = match duration_parts.get(1) {
        Some(min_str) => min_str.parse().map_err(|_| "Invalid format".to_string())?,
        None => 0,
    };

    let hours= match duration_parts.get(2) {
        Some(hour_str) => hour_str.parse().map_err(|_| "Invalid format".to_string())?,
        None => 0,
    };

    let duration_secs= hours * 3600 + minutes * 60 + seconds;
    Ok(Duration::from_secs(duration_secs))
}

pub struct RssTranscodizer {
    feed_url: Url,
    transcode_service_url: Url,
    should_transcode: bool,
}


impl RssTranscodizer {
    pub async fn new(url: Url, transcode_service_url: Url, should_transcode: bool) -> Self {

        Self { feed_url: url, transcode_service_url, should_transcode}
    }


    pub async fn transcodize(&self) -> eyre::Result<String> {

        cached_transcodize(TranscodeParams {
            transcode_service_url_str: self.transcode_service_url.clone().to_string(),
            feed_url: self.feed_url.to_owned(),
            should_transcode: self.should_transcode
        }).await
    }
}

#[derive(Clone, Hash)]
struct TranscodeParams {
    transcode_service_url_str: String,
    feed_url: Url,
    should_transcode: bool,
}

impl Display for TranscodeParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", &self.transcode_service_url_str, &self.feed_url)
    }
}

#[cfg_attr(not(test),
    io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<TranscodeParams, String>",
    create = r##" {
        AsyncRedisCache::new("cached_transcodizer=", 600)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
            .build()
            .await
            .expect("rss_transcodizer cache")
    } "##
))]
async fn cached_transcodize(input: TranscodeParams) -> eyre::Result<String> {
    let transcode_service_url = Url::parse(&input.transcode_service_url_str).unwrap();
    let rss_body = (async { reqwest::get(input.feed_url).await?.bytes().await }).await?;
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

    feed_builder.generator(Some("generated by vod2pod-rss".to_string()));



    let futures = FuturesUnordered::new();
    for entry in feed.entries.iter() {
        let ser_url = transcode_service_url.clone();
        futures.push(async move { convert_item(ConvertItemsParams { item: entry.to_owned(), transcode_service_url: ser_url, should_transcode: input.should_transcode}).await });
    }

    let item_futures: Vec<_> =  futures.collect().await;
    let mut feed_items: Vec<_> = item_futures.iter().filter_map(|x| if !x.is_none() { Some(x.to_owned().unwrap())} else { None }).collect();
    feed_items.sort_by(|a, b| {
        DateTime::parse_from_rfc2822(b.pub_date().unwrap_or_default()).unwrap_or_default()
            .cmp(
        &DateTime::parse_from_rfc2822(&a.pub_date().unwrap_or_default()).unwrap_or_default()
            )
    });
    feed_builder.items(feed_items);
    Ok(feed_builder.build().to_string())
}

struct ConvertItemsParams {
    item: Entry,
    transcode_service_url: Url,
    should_transcode: bool
}

async fn convert_item(params: ConvertItemsParams) -> Option<Item> {
    let item = params.item;
    let transcode_service_url = params.transcode_service_url;

    //needed for youtube: if views are 0 it indicates a premiere
    if let Some(community) = item.media.first().and_then(|media| media.community.as_ref()) {
        if community.stats_views == Some(0) {
            debug!("ignoring item with 0 views (probably youtube preview) with name: {:?}", item.title);
            return None;
        }
    }

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

    let description = get_description(&media_element, &item);
    item_builder.description(description);

    if let Some(x) = item.published {
        debug!("Published date found: {:?}", x.to_rfc2822());
        item_builder.pub_date(Some(x.to_rfc2822()));
    }

    if let Some(x) = &media_element.thumbnails.first() {
        debug!("Thumbnail image found: {:?}", x.image.uri);
        itunes_builder.image(Some(x.image.uri.clone()));
    }

    let duration = match media_element.duration {
        Some(x) => {
    debug!("duration found: {} secs", x.as_secs());
            x
        },
        None => {
            debug!("runnign regex on url: {}", media_url.as_str());
            let youtube_regex = regex::Regex::new(r#"^(https?://)?(www\.)?(youtu\.be/|youtube\.com/)"#).unwrap();
            if youtube_regex.is_match(media_url.as_str()) {
                let duration = match get_youtube_video_duration(&media_url).await {
                    Ok(dur) => Duration::from_secs(dur),
                    Err(e) => {
                        error!("Error getting youtube video duration: {:?}", e);
                        Duration::default()
                    }
                };
                duration
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
    let bitrate: u64 = conf().get(ConfName::Mp3Bitrate).unwrap()
        .parse()
        .expect("MP3_BITRATE must be a number");
    let generation_uuid  = uuid::Uuid::new_v4().to_string();
    transcode_service_url
        .query_pairs_mut()
        .append_pair("bitrate", bitrate.to_string().as_str())
        .append_pair("uuid", generation_uuid.as_str())
        .append_pair("duration", duration_secs.to_string().as_str())
        .append_pair("url", media_url.as_str())
        .append_pair("ext", ".mp3"); //this should allways be last, some players refuse to play urls not ending in .mp3

    let enclosure = Enclosure {
        length: (bitrate * 1024 * duration_secs).to_string(),
        url: transcode_service_url.to_string(),
        mime_type: "audio/mpeg".to_string(),
    };

    debug!("setting enclosure to: \nlength: {}, url: {}, mime_type: {}", enclosure.length, enclosure.url, enclosure.mime_type);

    let guid = generate_guid(&item.id);
    item_builder.guid(Some(guid));
    if params.should_transcode { item_builder.enclosure(Some(enclosure)); }

    item_builder.itunes_ext(Some(itunes_builder.build()));
    debug!("item parsing completed!\n------------------------------\n------------------------------");
    Some(item_builder.build())
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

fn get_description(media_element: &MediaObject, item: &Entry) -> Option<String> {
    const HEADER: &str = "";
    const FOOTER: &str = concat!(
        "<br><br>generated by vod2pod-rss ",
        env!("CARGO_PKG_VERSION"),
        " made by Mattia Di Eleuterio (<a href=\"https://github.com/madiele\">madiele</a>). Check out the <a href=\"https://github.com/madiele/vod2pod-rss\">GitHub repository</a>."
    );
    let mut description = None;

    if let Some(x) = &media_element.description {
        debug!("Description found: {:?}", x.content);
        description = Some(x.content.clone());
    }
    else if let Some(x) = &item.content {
        debug!("Description found: {:?}", x);
        description = Some(x.body.clone().unwrap_or_default());
    }
    else if let Some(x) = &item.summary {
        debug!("Description found: {:?}", x);
        description = Some(x.content.to_string().clone());
    }

    description.map(|desc| format!("{}{}{}", HEADER, desc, FOOTER))
}

#[cfg(test)]
mod test {
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

        let rss_url = Url::parse("http://127.0.0.1:9872/feed.rss").unwrap();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9872/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url, true).await;

        let res = validate_must_have_props(rss_transcodizer).await;

        stop_test(handle).await;

        res
    }

    #[actix_web::test]
    async fn rss_twitch_feed() -> Result<(), String> {
        let handle = startup_test("twitch".to_string(), 9871).await;

        let rss_url = Url::parse("http://127.0.0.1:9871/feed.rss").unwrap();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9871/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url, true).await;


        let res = validate_must_have_props(rss_transcodizer).await;

        stop_test(handle).await;

        res
    }

    #[actix_web::test]
    async fn rss_youtube_feed() -> Result<(), String> {
        let handle = startup_test("youtube".to_string(), 9870).await;

        let rss_url = Url::parse("http://127.0.0.1:9870/feed.rss").unwrap();
        println!("testing feed {rss_url}");
        let transcode_service_url = "http://127.0.0.1:9870/transcode".parse().unwrap();
        let rss_transcodizer = RssTranscodizer::new(rss_url, transcode_service_url, true).await;

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

    #[test]
    fn test_parse_duration_hhmmss() {
        assert_eq!(parse_duration("01:02:03").unwrap(), Duration::from_secs(3723));
    }

    #[test]
    fn test_parse_duration_mmss() {
        assert_eq!(parse_duration("30:45").unwrap(), Duration::from_secs(1845));
    }

    #[test]
    fn test_parse_duration_ss() {
        assert_eq!(parse_duration("15").unwrap(), Duration::from_secs(15));
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
    }

    #[test]
    fn test_parse_duration_wrong_format() {
        assert_eq!(parse_duration("invalid").unwrap_err(), "Invalid format".to_string());
    }
}
