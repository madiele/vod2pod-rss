use async_trait::async_trait;
use feed_rs::model::Entry;
use regex::Regex;
use reqwest::Url;

use crate::configs::{conf, Conf, ConfName};

use super::MediaProvider;

pub(super) struct GenericProvider {
    url: Url
}

#[async_trait]
impl MediaProvider for GenericProvider {
    fn new(url: &Url) -> Self { GenericProvider { url: url.clone() } }

    async fn get_item_duration(&self, _url: &Url) -> eyre::Result<Option<u64>> { Ok(None) }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        Ok(media_url.to_owned())
    }

    async fn filter_item(&self, _rss_item: &Entry) -> bool { false }

    fn media_url_regexes(&self) -> Vec<Regex> {
        let generic_whitelist = get_generic_whitelist();

        #[cfg(not(test))]
        return generic_whitelist;
        #[cfg(test)] //this will allow test to use localhost ad still work
        return [generic_whitelist, vec!(regex::Regex::new(r"^http://127\.0\.0\.1:9872").unwrap())].concat();
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> { get_generic_whitelist() }

    async fn feed_url(&self) -> eyre::Result<Url> { Ok(self.url.clone()) }
}

fn get_generic_whitelist() -> Vec<Regex> {
    let binding = conf().get(ConfName::ValidUrlDomains).unwrap();
    let patterns: Vec<&str> = binding.split(",").filter(|e| e.trim().len() > 0).collect();

    let mut regexes: Vec<Regex> = Vec::with_capacity(patterns.len() + 1);
    for pattern in patterns {
        regexes.push(Regex::new(&pattern.to_string().replace(".", "\\.").replace("*", ".+")).unwrap())
    }

    let match_extensions = regex::Regex::new("^(https?://)?.+\\.(mp3|mp4|wav|avi|mov|flv|wmv|mkv|aac|ogg|webm|3gp|3g2|asf|m4a|mpg|mpeg|ts|m3u|m3u8|pls)$").unwrap();
    regexes.push(match_extensions);

    return regexes;
}

