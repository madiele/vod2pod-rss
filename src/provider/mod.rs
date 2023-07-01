use std::time::Duration;
#[allow(unused_imports)]
use cached::AsyncRedisCache;
#[allow(unused_imports)]
use cached::proc_macro::io_cached;

use async_trait::async_trait;
use eyre::eyre;
use feed_rs::model::Entry;
use log::{debug, warn, info};
use regex::Regex;
use reqwest::Url;
use tokio::process::Command;

use crate::configs::{conf, ConfName, Conf};

macro_rules! dispatch_if_match {
    ($url: expr, $provider: ident) => {
        let provider = $provider::new($url);
        for regex in provider.domain_whitelist_regexes() {
            if regex.is_match(&$url.to_string()) {
                return Box::new(provider);
            }
        }
    };
}

pub fn new(url: &Url) -> Box<dyn MediaProvider> {
    dispatch_if_match!(url, YoutubeProvider);
    dispatch_if_match!(url, TwitchProvider);

    return Box::new(GenericProvider { url: url.clone() })
}

/// This trait rappresent a provider offering a media stream (youtube, twitch, etc...)
/// You can implement your own by implement this trait and edit the from(...) fn in the provider module
/// to add a new provider
///
/// # Examples
///
/// ```
/// use vod2pod_rss::provider::MediaProvider;
/// use async_trait::async_trait;
/// use feed_rs::model::Entry;
/// use regex::Regex;
/// use reqwest::Url;

/// struct GenericProvider {
/// }
///
/// #[async_trait]
/// impl MediaProvider for GenericProvider {
///     fn new(_url: &Url) -> Self {
///         GenericProvider { }
///     }
///     async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> {
///         Ok(None)
///     }
///     async fn filter_item(&self, _rss_item: Entry) -> bool {
///         false
///     }
///     fn media_url_regex() -> Vec<Regex> {
///         vec!()
///     }
///     fn domain_whitelist_regex() -> Vec<Regex> {
///         vec!()
///     }
/// }
/// ```
#[async_trait]
pub trait MediaProvider {
    /// Retrieves the duration of an item from the given URL.
    /// Not needed if the duration is already in the rss feed offered from the provider,
    /// in this case implement this to return None
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the item.
    ///
    /// # Examples
    ///
    /// TODO
    async fn get_item_duration(&self, media_url: &Url) -> eyre::Result<Option<u64>>;

    /// Run on each rss item and decides if item should be ignored
    ///
    /// # Arguments
    ///
    /// * `rss_item` - The RSS item to filter.
    ///
    /// # Examples
    ///
    /// TODO
    async fn filter_item(&self, rss_item: Entry) -> bool;

    /// Returns the regular expressions for matching media URLs.
    ///
    /// # Examples
    ///
    /// TODO
    fn media_url_regex(&self) -> Vec<Regex>;

    /// Returns the regular expressions that will match all urls offered by the provider.
    /// This are the url associated with the provider es:
    /// for youtube you would need to match https://youtube\.com, https://youtu\.be, and https://.*\.googlevideo\.com/ (used to host the videos).
    /// If this returns None then you would need to use the VALID_URL_DOMAINS env variable to add the allowed domains
    ///
    /// # Examples
    ///
    /// TODO
    fn domain_whitelist_regexes(&self) -> Vec<Regex>;

    /// Constructs the struct for the MediaProvider
    ///
    /// # Examples
    ///
    /// TODO
    fn new(url: &Url) -> Self where Self: Sized;

    /// if possible this will return the url of the rss/atom feed
    /// this is run only during feed generation and should error if the provider is called with a media url
    ///
    ///
    /// # Examples
    ///
    /// TODO
    async fn feed_url(&self) -> eyre::Result<Url>;
}

struct GenericProvider {
    url: Url
}

#[async_trait]
impl MediaProvider for GenericProvider {
    fn new(url: &Url) -> Self { GenericProvider { url: url.clone() } }

    async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> { Ok(None) }

    async fn filter_item(&self, _rss_item: Entry) -> bool { false }

    fn media_url_regex(&self) -> Vec<Regex> { get_generic_whitelist() }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> { get_generic_whitelist() }

    async fn feed_url(&self) -> eyre::Result<Url> { Ok(self.url.clone()) }
}

fn get_generic_whitelist() -> Vec<Regex> {
        let binding = conf().get(ConfName::ValidUrlDomains).unwrap();
        let patterns: Vec<&str> = binding.split(",").collect();

        let mut regexes: Vec<Regex> = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            regexes.push(Regex::new(&pattern.to_string().replace("*", ".+").replace(".", "\\.")).unwrap())
        }

        return regexes;
}

struct YoutubeProvider {
    url: Url
}

#[async_trait]
impl MediaProvider for YoutubeProvider {

    async fn get_item_duration(&self, url: &Url) -> eyre::Result<Option<u64>> {
        get_youtube_video_duration(url).await
    }

    async fn filter_item(&self, rss_item: Entry) -> bool {
        if let Some(community) = rss_item.media.first().and_then(|media| media.community.as_ref()) {
            if community.stats_views == Some(0) {
                debug!("ignoring item with 0 views (probably youtube preview) with name: {:?}", rss_item.title);
                return true;
            }
        }
        return false;
    }

    fn media_url_regex(&self) -> Vec<Regex> {
        return vec!(regex::Regex::new(r"^(https?://)?(www\.youtube\.com|youtu\.be)/.+$").unwrap());
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        return vec!(
            regex::Regex::new(r"^https://.*\.youtube\.com/").unwrap(),
            regex::Regex::new(r"^https://youtube\.com/").unwrap(),
            regex::Regex::new(r"^https://youtu\.be/").unwrap(),
            regex::Regex::new(r"^https://.*\.youtu\.be/").unwrap(),
            regex::Regex::new(r"^https://.*\.googlevideo\.com/").unwrap(),
        );
    }
    fn new(url: &Url) -> Self {
        YoutubeProvider { url: url.clone() }
    }

    async fn feed_url(&self) -> eyre::Result<Url> {
        match self.url.path() {
            path if path.starts_with("/playlist") => feed_url_for_yt_playlist(&self.url).await,
            path if path.starts_with("/feeds/") => feed_url_for_yt_atom(&self.url).await,
            path if path.starts_with("/channel/") => feed_url_for_yt_channel(&self.url).await,
            path if path.starts_with("/user/") => feed_url_for_yt_channel(&self.url).await,
            path if path.starts_with("/c/") => feed_url_for_yt_channel(&self.url).await,
            path if path.starts_with("/@") => feed_url_for_yt_channel(&self.url).await,
            _ => Err(eyre!("unsupported youtube url")),
        }
    }
}

#[cfg_attr(not(test),
io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, Option<u64>>",
    create = r##" {
        AsyncRedisCache::new("cached_yt_video_duration=", 86400)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
            .build()
            .await
            .expect("youtube_duration cache")
} "##
))]
async fn get_youtube_video_duration(url: &Url) -> eyre::Result<Option<u64>> {
    debug!("getting duration for yt video: {}", url);

    if let Ok(_) = conf().get(ConfName::YoutubeApiKey) {
        let id = url.query_pairs().find(|(key, _)| key == "v").map(|(_, value)| {value}).unwrap().to_string();
        Ok(Some(get_yotube_duration_with_apikey(&id).await?.as_secs()))
    } else {
        acquire_semaphore("yt_duration_semaphore".to_string(), url.to_string()).await?;
        let output = Command::new("yt-dlp")
            .arg("--get-duration")
            .arg(url.to_string())
            .output()
        .await;
        release_semaphore("yt_duration_semaphore".to_string(), url.to_string()).await?;
        if let Ok(x) = output {
            let duration_str = std::str::from_utf8(&x.stdout).unwrap().trim().to_string();
            Ok(Some(parse_duration(&duration_str).unwrap_or_default().as_secs()))
        } else {
            warn!("could not parse youtube video duration");
            Ok(Some(0))
        }
    }
}

async fn feed_url_for_yt_playlist(url: &Url) -> eyre::Result<Url> {
    let playlist_id = url.query_pairs()
        .find(|(key, _)| key == "list")
        .map(|(_, value)| value)
        .ok_or_else(|| eyre::eyre!("Failed to parse playlist ID from URL: {}", url))?;

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
async fn feed_url_for_yt_atom(url: &Url) -> eyre::Result<Url> {
        Ok(url.clone())
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
async fn feed_url_for_yt_channel(url: &Url) -> eyre::Result<Url> {
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

struct TwitchProvider {
    url: Url
}

#[async_trait]
impl MediaProvider for TwitchProvider {

    fn new(url: &Url) -> Self {
        TwitchProvider { url: url.clone()}
    }

    async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> {
        Ok(None)
    }

    async fn filter_item(&self, _rss_item: Entry) -> bool {
        false
    }

    fn media_url_regex(&self) -> Vec<Regex> {
        return vec!(
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap()
        );
    }
    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        return vec!(
            regex::Regex::new(r"^https?://(.*\.)?twitch\.tv/").unwrap(),
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap()
        );
    }
    async fn feed_url(&self) -> eyre::Result<Url> {
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
