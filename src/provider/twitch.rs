use async_trait::async_trait;
use feed_rs::model::Entry;
use log::info;
use regex::Regex;
use reqwest::Url;

use crate::configs::{conf, ConfName, Conf};

use super::MediaProvider;

pub(super) struct TwitchProvider {
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

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        Ok(media_url.to_owned())
    }

    async fn filter_item(&self, _rss_item: &Entry) -> bool {
        false
    }

    fn media_url_regexes(&self) -> Vec<Regex> {
        return vec!(
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap()
        );
    }
    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        let twitch_whitelist =  vec!(
            regex::Regex::new(r"^https?://(.*\.)?twitch\.tv/").unwrap(),
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap(),
            regex::Regex::new(r"^http://ttprss(:[0-9]+)?/").unwrap(),
            regex::Regex::new(r"^http://localhost:8085/vod/").unwrap(),
        );
        #[cfg(not(test))]
        return twitch_whitelist;
        #[cfg(test)] //this will allow test to use localhost ad still work
        return [twitch_whitelist, vec!(regex::Regex::new(r"^http://127\.0\.0\.1:9871").unwrap())].concat();
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

