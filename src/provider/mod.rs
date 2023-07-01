mod generic;
mod youtube;
mod twitch;

use async_trait::async_trait;
use feed_rs::model::Entry;
use log::debug;
use regex::Regex;
use reqwest::Url;

use crate::provider::{generic::GenericProvider, youtube::YoutubeProvider, twitch::TwitchProvider};

macro_rules! dispatch_if_match {
    ($url: expr, $provider: ident) => {
        let provider = $provider::new($url);
        for regex in provider.domain_whitelist_regexes() {
            if regex.is_match(&$url.to_string()) {
                debug!("using {}" , stringify!($provider));
                return Box::new(provider);
            }
        }
    };
}

pub fn from(url: &Url) -> Box<dyn MediaProvider> {
    dispatch_if_match!(url, YoutubeProvider);
    dispatch_if_match!(url, TwitchProvider);
    debug!("using GenericProvider as provider");
    return Box::new(GenericProvider::new(url))
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
///     url: Url
/// }
///
/// #[async_trait]
/// impl MediaProvider for GenericProvider {
///     fn new(url: &Url) -> Self { GenericProvider { url: url.clone() } }
///
///     async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> { Ok(None) }
///
///     async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
///         Ok(media_url.to_owned())
///     }
///
///     async fn filter_item(&self, _rss_item: &Entry) -> bool { false }
///
///     fn media_url_regexes(&self) -> Vec<Regex> { vec!() }
///
///     fn domain_whitelist_regexes(&self) -> Vec<Regex> { vec!() }
///
///     async fn feed_url(&self) -> eyre::Result<Url> { Ok(self.url.clone()) }
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

    /// Takes an URL and returns the stream URL
    /// Only run when trancoding, if URL can't be converted to a sreamable URL will return an error
    ///
    /// example: https://www.youtube.com/watch?v=UMO52N2vfk0 -> https://googlevideo.com/....
    ///
    /// # Arguments
    ///
    /// * `media_url` - The original URL found inside the RSS that should be streamed.
    ///
    /// # Examples
    ///
    /// TODO
    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url>;

    /// Run on each rss item and decides if item should be ignored
    ///
    /// # Arguments
    ///
    /// * `rss_item` - The RSS item to filter.
    ///
    /// # Examples
    ///
    /// TODO
    async fn filter_item(&self, rss_item: &Entry) -> bool;

    /// Returns the regular expressions for matching media URLs.
    ///
    /// # Examples
    ///
    /// TODO
    fn media_url_regexes(&self) -> Vec<Regex>;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_twitch_to_feed() {
        temp_env::async_with_vars([
            ("TWITCH_TO_PODCAST_URL", Some("http://ttprss")),
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(feed_url.as_str(), "http://ttprss/vod/andersonjph?transcode=false");
        }
    }

    #[tokio::test]
    async fn test_http_ttprss_twitch_to_feed() {
        temp_env::async_with_vars([
            ("TWITCH_TO_PODCAST_URL", Some("http://ttprss")),
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(feed_url.as_str(), "http://ttprss/vod/andersonjph?transcode=false");
        }
    }

    #[tokio::test]
    async fn test_ssl_ttprss_twitch_to_feed() {
        temp_env::async_with_vars([
            ("TWITCH_TO_PODCAST_URL", Some("http://ttprss")),
        ], test()).await;

        async fn test() {
            std::env::set_var("TWITCH_TO_PODCAST_URL", "https://ttprss");
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(feed_url.as_str(), "https://ttprss/vod/andersonjph?transcode=false");
        }
    }


    #[tokio::test]
    async fn test_youtube_to_feed() {
        temp_env::async_with_vars([
            ("YT_API_KEY", Some("")),
            ("PODTUBE_URL", None)
        ], test()).await;

        async fn test() {
            let url = Url::parse("https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
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
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?playlist_id=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD");
        }
    }

    #[tokio::test]
    async fn test_youtube_to_feed_already_atom_feed() {
        let url = Url::parse("https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
        println!("url: {:?}", url);
        let feed_url = from(&url).feed_url().await.unwrap();
        println!("feed_url: {:?}", feed_url);
        assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw");
    }

    #[tokio::test]
    async fn test_generic_to_feed() {
        let url = Url::parse("https://feeds.simplecast.com/aU_RzZ7j").unwrap();
        println!("url: {:?}", url);
        let feed_url = from(&url).feed_url().await.unwrap();
        println!("feed_url: {:?}", feed_url);
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
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
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
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(feed_url.as_str(), "http://podtube/youtube/playlist/PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD");
        }
    }
}
