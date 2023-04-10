use async_trait::async_trait;
use log::info;
use reqwest::Url;
use tokio::process::Command;
#[allow(unused_imports)]
use cached::AsyncRedisCache;
#[allow(unused_imports)]
use cached::proc_macro::io_cached;

pub fn from(url: Url) -> Box<dyn ConvertableToFeed> {
    let url = url.to_owned();
    match url {
        _ if url.domain().unwrap_or_default().contains("twitch.tv") => Box::new(TwitchUrl { url }),
        _ if url.domain().unwrap_or_default().contains("youtube.com") || url.domain().unwrap_or_default().contains("youtu.be") => Box::new(YoutubeUrl { url }),
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
        let mut ttprss_url = std::env::var("TWITCH_TO_PODCAST_URL")?;
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


struct YoutubeUrl {
    url: Url,
}

#[cfg_attr(not(test),
io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, Url>",
    create = r##" {
        let redis_address = std::env::var("REDIS_ADDRESS").unwrap_or_else(|_| "localhost".to_string());
        let redis_port = std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());

        AsyncRedisCache::new("youtube_channel_username_to_id=", 9999999)
            .set_refresh(false)
            .set_connection_string(&format!("redis://{}:{}/", redis_address, redis_port))
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
impl ConvertableToFeed for YoutubeUrl {
    async fn to_feed_url(&self) -> eyre::Result<Url> {
        info!("trying to convert youtube channel url {}", self.url);
        let url_with_channel_id = find_yt_channel_url_with_c_id(&self.url).await?;
        let channel_id = url_with_channel_id.path_segments().unwrap().last().unwrap();
        let mut feed_url = Url::parse("https://www.youtube.com/feeds/videos.xml")?;
        feed_url.query_pairs_mut().append_pair("channel_id", channel_id);
        info!("converted to {feed_url}");
        Ok(feed_url)
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
        let url = "https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw";
        println!("{:?}", url);
        let channel = YoutubeUrl { url: Url::parse(url).unwrap() };
        let feed_url = channel.to_feed_url().await.unwrap();
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
}
