use async_trait::async_trait;
use regex::Regex;
use reqwest::Url;

use crate::configs::{conf, Conf, ConfName};

use super::MediaProvider;

pub struct GenericProvider;

#[async_trait]
impl MediaProvider for GenericProvider {
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        Ok(reqwest::get(channel_url).await?.text().await?)
    }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        Ok(media_url.to_owned())
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        get_generic_whitelist()
    }
}

fn get_generic_whitelist() -> Vec<Regex> {
    let binding = conf().get(ConfName::ValidUrlDomains).unwrap();
    let patterns: Vec<&str> = binding
        .split(',')
        .filter(|e| !e.trim().is_empty())
        .collect();

    let mut regexes: Vec<Regex> = Vec::with_capacity(patterns.len() + 1);
    for pattern in patterns {
        regexes
            .push(Regex::new(&pattern.to_string().replace('.', "\\.").replace('*', ".+")).unwrap())
    }

    let match_extensions = regex::Regex::new("^(https?://)?.+\\.(mp3|mp4|wav|avi|mov|flv|wmv|mkv|aac|ogg|webm|3gp|3g2|asf|m4a|mpg|mpeg|ts|m3u|m3u8|pls)$").unwrap();
    regexes.push(match_extensions);

    regexes
}

