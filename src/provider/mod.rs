mod generic;
mod peertube;
mod twitch;
mod youtube;

use async_trait::async_trait;
use feed_rs::model::Entry;
use log::debug;
use regex::Regex;
use reqwest::Url;
use rss::extension::itunes::ITunesChannelExtensionBuilder;

use crate::provider::{
    generic::GenericProvider, peertube::PeertubeProvider, twitch::TwitchProvider,
    youtube::YoutubeProvider,
};

macro_rules! dispatch_if_match {
    ($url: expr, $provider: ident) => {
        let provider = $provider::new($url);
        for regex in provider.domain_whitelist_regexes() {
            if regex.is_match(&$url.to_string()) {
                debug!("using {}", stringify!($provider));
                return Box::new(provider);
            }
        }
    };
}

pub fn from(url: &Url) -> Box<dyn MediaProvider> {
    dispatch_if_match!(url, YoutubeProvider);
    dispatch_if_match!(url, TwitchProvider);
    dispatch_if_match!(url, PeertubeProvider);
    debug!("using GenericProvider as provider");
    return Box::new(GenericProvider::new(url));
}

/// This trait rappresent a provider offering a media stream (youtube, twitch, etc...).
/// to add a new provider impelment this trait and add `dispatch_if_match!(url, YourProvider);` inside
/// src/provider/mod.rs -> from(...)
/// RSS feed should be generated in a separate webserver and then added inside the docker-compose.yml
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
    /// * `media_url` - The URL of the item.
    async fn get_item_duration(&self, media_url: &Url) -> eyre::Result<Option<u64>>;

    /// Takes an URL and returns the stream URL, this will be passed to ffmpeg to start the
    /// transcoding process
    /// Only run when trancoding, if URL can't be converted to a streamable URL will return an error
    ///
    /// example: https://www.youtube.com/watch?v=UMO52N2vfk0 -> https://googlevideo.com/....
    ///
    /// for some provider conversion might not be needed, in that case just return the input
    ///
    /// # Arguments
    ///
    /// * `media_url` - The original URL found inside the RSS that should be streamed.
    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url>;

    /// Run on each rss item and decides if item should be ignored
    ///
    /// # Arguments
    ///
    /// * `rss_item` - The RSS item to filter.
    /// # Examples
    /// The YoutubeProvider uses this to filter out premieres
    async fn filter_item(&self, rss_item: &Entry) -> bool;

    /// Returns the regular expressions for matching media URLs during RSS/atom parsing.
    /// this will eventually be passed to the get_stream_url()
    fn media_url_regexes(&self) -> Vec<Regex>;

    /// Returns the regular expressions that will match all urls offered by the provider.
    /// This are the url associated with the provider
    /// es: for youtube you would need to match
    /// https://youtube\.com, https://youtu\.be, and https://.*\.googlevideo\.com/ (used to host the videos).
    ///
    /// if you need this to be user configurable then you need to create a ENV var, check the GenericProvider
    /// implementation for hints on how to do it
    ///
    /// # IMPORTANT
    /// this list is used to dinamically dispatch the provider, so the regex written here should
    /// never match what is already matched by other providers, if this for some reason is a huge
    /// limitation open an issue with a change request and why you need it to change.
    /// Also be warned missing a match here will cause the server to use the GenericProvider instead
    fn domain_whitelist_regexes(&self) -> Vec<Regex>;

    /// Constructs the struct for the MediaProvider
    ///
    /// # Arguments
    ///
    /// * `url` -   The URL pointing to the feed URL (this can be both the original public feed URL
    ///             or the URL of the internal feed generation service (es: podtube, ttprss))
    ///
    /// # Returns
    ///
    /// The constructed struct for the media provider.
    ///
    /// Only used for dynamic dispatching
    fn new(url: &Url) -> Self
    where
        Self: Sized;

    /// This will return the url of the rss/atom feed
    /// this is run only during feed generation and normally should use the url given by the new(...)
    /// for conversion
    /// es: youtube channel url -> url to the rss feed of it
    async fn feed_url(&self) -> eyre::Result<Url>;
}

#[async_trait]
pub trait MediaProviderV2 {
    /// Given a channel will generate the full RRS feed
    ///
    /// # Arguments
    ///
    /// * `channel_url` - The URL of channel for wich the rss will be generated.
    /// * `transcode_service_url` - the absolute URL to the trancode service, needed to set the enclosure correctly if trancoding is needed
    async fn generate_rss_feed(
        &self,
        channel_url: Url,
        transcode_service_url: Option<Url>,
    ) -> eyre::Result<String>;

    /// Takes an URL and returns the stream URL, this will be passed to ffmpeg to start the
    /// transcoding process
    /// Only run when trancoding, if URL can't be converted to a streamable URL will return an error
    ///
    /// example: https://www.youtube.com/watch?v=UMO52N2vfk0 -> https://googlevideo.com/....
    ///
    /// for some provider conversion might not be needed, in that case just return the input
    ///
    /// # Arguments
    ///
    /// * `media_url` - The original URL found inside the RSS that should be streamed.
    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url>;

    /// Returns the regular expressions that will match all urls offered by the provider.
    /// This are the url associated with the provider
    /// es: for youtube you would need to match
    /// https://youtube\.com, https://youtu\.be, and https://.*\.googlevideo\.com/ (used to host the videos).
    ///
    /// if you need this to be user configurable then you need to create a ENV var, check the GenericProvider
    /// implementation for hints on how to do it
    ///
    /// # IMPORTANT
    /// this list is used to dinamically dispatch the provider, so the regex written here should
    /// never match what is already matched by other providers, if this for some reason is a huge
    /// limitation open an issue with a change request and why you need it to change.
    /// Also be warned missing a match here will cause the server to use the GenericProvider instead
    fn domain_whitelist_regexes(&self) -> Vec<Regex>;

    /// Constructs the struct for the MediaProvider
    ///
    /// # Arguments
    ///
    /// * `url` -   The URL pointing to the feed URL (this can be both the original public feed URL
    ///             or the URL of the internal feed generation service (es: podtube, ttprss))
    ///
    /// # Returns
    ///
    /// The constructed struct for the media provider.
    ///
    /// Only used for dynamic dispatching
    fn new() -> Self
    where
        Self: Sized;
}

pub fn build_default_rss_structure() -> rss::ChannelBuilder {
    let mut feed_builder = rss::ChannelBuilder::default();

    let mut namespaces = std::collections::BTreeMap::new();
    namespaces.insert(
        "rss".to_string(),
        "http://www.itunes.com/dtds/podcast-1.0.dtd".to_string(),
    );
    namespaces.insert(
        "itunes".to_string(),
        "http://www.itunes.com/dtds/podcast-1.0.dtd".to_string(),
    );
    feed_builder.namespaces(namespaces);

    feed_builder.generator(Some("generated by vod2pod-rss".to_string()));

    let mut itunes_section = ITunesChannelExtensionBuilder::default();
    itunes_section.block(Some("yes".to_string())); //this tells podcast players to not index the podcast in their search engines
    feed_builder.itunes_ext(Some(itunes_section.build()));

    feed_builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_twitch_to_feed() {
        temp_env::async_with_vars([("TWITCH_TO_PODCAST_URL", Some("http://ttprss"))], test()).await;

        async fn test() {
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "http://ttprss/vod/andersonjph?transcode=false"
            );
        }
    }

    #[tokio::test]
    async fn test_http_ttprss_twitch_to_feed() {
        temp_env::async_with_vars([("TWITCH_TO_PODCAST_URL", Some("http://ttprss"))], test()).await;

        async fn test() {
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "http://ttprss/vod/andersonjph?transcode=false"
            );
        }
    }

    #[tokio::test]
    async fn test_ssl_ttprss_twitch_to_feed() {
        temp_env::async_with_vars([("TWITCH_TO_PODCAST_URL", Some("http://ttprss"))], test()).await;

        async fn test() {
            std::env::set_var("TWITCH_TO_PODCAST_URL", "https://ttprss");
            let url = Url::parse("https://www.twitch.tv/andersonjph").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "https://ttprss/vod/andersonjph?transcode=false"
            );
        }
    }

    #[tokio::test]
    async fn test_youtube_to_feed() {
        temp_env::async_with_vars([("YT_API_KEY", Some("")), ("PODTUBE_URL", None)], test()).await;

        async fn test() {
            let url =
                Url::parse("https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw"
            );
        }
    }

    #[tokio::test]
    async fn test_youtube_playlist_to_feed() {
        temp_env::async_with_vars([("YT_API_KEY", Some("")), ("PODTUBE_URL", None)], test()).await;

        async fn test() {
            let url = Url::parse(
                "https://www.youtube.com/playlist?list=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD",
            )
            .unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(feed_url.as_str(), "https://www.youtube.com/feeds/videos.xml?playlist_id=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD");
        }
    }

    #[tokio::test]
    async fn test_youtube_to_feed_already_atom_feed() {
        let url = Url::parse(
            "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw",
        )
        .unwrap();
        println!("url: {:?}", url);
        let feed_url = from(&url).feed_url().await.unwrap();
        println!("feed_url: {:?}", feed_url);
        assert_eq!(
            feed_url.as_str(),
            "https://www.youtube.com/feeds/videos.xml?channel_id=UC-lHJZR3Gqxm24_Vd_AJ5Yw"
        );
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
        temp_env::async_with_vars(
            [
                ("YT_API_KEY", Some("1234567890123456780")),
                ("PODTUBE_URL", Some("http://podtube")),
            ],
            test(),
        )
        .await;

        async fn test() {
            let url =
                Url::parse("https://www.youtube.com/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw").unwrap();
            println!("url: {:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("feed_url: {:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "http://podtube/youtube/channel/UC-lHJZR3Gqxm24_Vd_AJ5Yw"
            );
        }
    }

    #[tokio::test]
    async fn test_youtube_playlist_to_feed_api_key_present() {
        temp_env::async_with_vars(
            [
                ("YT_API_KEY", Some("1234567890123456780")),
                ("PODTUBE_URL", Some("http://podtube")),
            ],
            test(),
        )
        .await;

        async fn test() {
            let url = Url::parse(
                "https://www.youtube.com/playlist?list=PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD",
            )
            .unwrap();
            println!("{:?}", url);
            let feed_url = from(&url).feed_url().await.unwrap();
            println!("{:?}", feed_url);
            assert_eq!(
                feed_url.as_str(),
                "http://podtube/youtube/playlist/PLm323Lc7iSW9Qw_iaorhwG2WtVXuL9WBD"
            );
        }
    }
}

