use async_trait::async_trait;
use feed_rs::model::Entry;
use regex::Regex;
use reqwest::Url;

pub fn from(url: &Url) -> Box<dyn MediaProvider> {
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
///     pub url: Url
/// }
///
/// #[async_trait]
/// impl MediaProvider for GenericProvider {
///     async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> {
///         Ok(None)
///     }
///     async fn filter_item(&self, _rss_item: Entry) -> bool {
///         false
///     }
///     fn media_url_regex(&self) -> Option<Regex> {
///         None
///     }
///     fn domain_whitelist_regex(&self) -> Option<Regex> {
///         None
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

    /// Returns the regular expression for matching media URLs.
    ///
    /// # Examples
    ///
    /// TODO
    fn media_url_regex(&self) -> Option<Regex>;

    /// Returns the regular expression that will match all urls offered by the provider.
    /// This are the url associated with the provider es:
    /// for youtube you would need to match https://youtube\.com, https://youtu\.be, and https://.*\.googlevideo\.com/ (used to host the videos).
    /// If this returns None then you would need to use the VALID_URL_DOMAINS env variable to add the allowed domains
    ///
    /// # Examples
    ///
    /// TODO
    fn domain_whitelist_regex(&self) -> Option<Regex>;
}

struct GenericProvider {
    pub url: Url
}

#[async_trait]
impl MediaProvider for GenericProvider {
    async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> {
        Ok(None)
    }
    async fn filter_item(&self, _rss_item: Entry) -> bool {
        false
    }
    fn media_url_regex(&self) -> Option<Regex> {
        None
    }
    fn domain_whitelist_regex(&self) -> Option<Regex> {
        None
    }
}
