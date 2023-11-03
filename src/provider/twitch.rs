use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset};
use feed_rs::model::Entry;
use log::{debug, info, warn};
use regex::Regex;
use reqwest::Url;
use rss::{
    extension::itunes::{ITunesChannelExtensionBuilder, ITunesItemExtensionBuilder},
    Channel, ChannelBuilder, GuidBuilder, Item, ItemBuilder,
};
use serde::Deserialize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::configs::{conf, Conf, ConfName};

use super::{MediaProvider, MediaProviderV2};

pub(super) struct TwitchProvider {
    url: Url,
}

#[async_trait]
impl MediaProvider for TwitchProvider {
    fn new(url: &Url) -> Self {
        TwitchProvider { url: url.clone() }
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
        return vec![regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap()];
    }
    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        let twitch_whitelist = vec![
            regex::Regex::new(r"^https?://(.*\.)?twitch\.tv/").unwrap(),
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap(),
            regex::Regex::new(r"^http://ttprss(:[0-9]+)?/").unwrap(),
            regex::Regex::new(r"^http://localhost:8085/vod/").unwrap(),
        ];
        #[cfg(not(test))]
        return twitch_whitelist;
        #[cfg(test)] //this will allow test to use localhost ad still work
        return [
            twitch_whitelist,
            vec![regex::Regex::new(r"^http://127\.0\.0\.1:9871").unwrap()],
        ]
        .concat();
    }
    async fn feed_url(&self) -> eyre::Result<Url> {
        info!("trying to convert twitch channel url {}", self.url);
        let username = self
            .url
            .path_segments()
            .ok_or_else(|| eyre::eyre!("Unable to get path segments"))?
            .last()
            .ok_or_else(|| eyre::eyre!("Unable to get last path segment"))?;
        let mut ttprss_url = conf().get(ConfName::TwitchToPodcastUrl)?;
        if !ttprss_url.starts_with("http://") && !ttprss_url.starts_with("https://") {
            ttprss_url = format!("http://{}", ttprss_url);
        }
        let mut feed_url =
            Url::parse(&format!("{}{}", ttprss_url, "/vod")).map_err(|err| eyre::eyre!(err))?;
        feed_url
            .path_segments_mut()
            .map_err(|_| eyre::eyre!("Unable to get mutable path segments"))?
            .push(username);
        feed_url.query_pairs_mut().append_pair("transcode", "false");
        info!("converted to {feed_url}");
        Ok(feed_url)
    }
}

struct TwitchProviderV2 {}

#[async_trait]
impl MediaProviderV2 for TwitchProviderV2 {
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        todo!()
    }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        get_twitch_stream_url(media_url).await
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        let twitch_whitelist = vec![
            regex::Regex::new(r"^https?://(.*\.)?twitch\.tv/").unwrap(),
            regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap(),
        ];
        #[cfg(not(test))]
        return twitch_whitelist;
        #[cfg(test)] //this will allow test to use localhost ad still work
        return [
            twitch_whitelist,
            vec![regex::Regex::new(r"^http://127\.0\.0\.1:9871").unwrap()],
        ]
        .concat();
    }

    fn new() -> Self
    where
        Self: Sized,
    {
        TwitchProviderV2 {}
    }
}

async fn get_twitch_stream_url(url: &Url) -> eyre::Result<Url> {
    debug!("getting stream_url for twitch video: {}", url);
    let output = tokio::process::Command::new("yt-dlp")
        .arg("-f")
        .arg("bestaudio")
        .arg("--get-url")
        .arg(url.as_str())
        .output()
        .await;

    match output {
        Ok(x) => {
            let raw_url = std::str::from_utf8(&x.stdout).unwrap_or_default();
            match Url::from_str(raw_url) {
                Ok(url) => Ok(url),
                Err(e) => {
                    warn!(
                        "error while parsing stream url:\ninput: {}\nerror: {}",
                        raw_url,
                        e.to_string()
                    );
                    Err(eyre::eyre!(e))
                }
            }
        }
        Err(e) => Err(eyre::eyre!(e)),
    }
}

#[derive(Debug)]
pub struct OAuthCredentials {
    oauth_token: String,
    oauth_expire_epoch: u64,
}

pub async fn authorize(client_id: &str, client_secret: &str) -> eyre::Result<OAuthCredentials> {
    let base_url = "https://id.twitch.tv/oauth2/token";
    let client = reqwest::Client::new();

    let body = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "client_credentials"),
    ];

    let mut attempt = 0;
    loop {
        let response = client.post(base_url).form(&body).send().await?;

        if response.status().is_success() {
            let body = response.text().await?;
            let credentials: Value = serde_json::from_str(&body)?;

            let oauth_token = credentials["access_token"]
                .as_str()
                .ok_or(eyre::eyre!("no access token in twitch response"))?
                .to_string();
            let oauth_expire_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
                + credentials["expires_in"]
                    .as_u64()
                    .ok_or(eyre::eyre!("no expires_in in twitch response"))?;

            return Ok(OAuthCredentials {
                oauth_token,
                oauth_expire_epoch,
            });
        }

        attempt += 1;
        if attempt >= 3 {
            return Err(eyre::eyre!(
                "Could not get OAuth token from Twitch, reason: {}",
                response.text().await?
            ));
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct User {
    login: String,
    display_name: String,
    profile_image_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Vod {
    id: String,
    thumbnail_url: Option<String>,
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    stream_id: Option<String>,
    created_at: Option<String>,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Stream {
    id: String,
}

fn build_items_from_vods(items: Vec<Vod>) -> Vec<Item> {
    let rss_item: Vec<Item> = items
        .into_iter()
        .filter_map(|vod| {
            let video_id = vod.id;
            let title = vod.title.unwrap_or("".to_owned());
            let description = title.clone();
            let published_at = vod.created_at.unwrap_or("".to_owned());
            let mut item_builder = ItemBuilder::default();
            item_builder.title(Some(title.clone()));
            item_builder.description(Some(description.clone()));
            item_builder.link(Some(video_id.clone()));
            item_builder.guid(Some(GuidBuilder::default().value(video_id.clone()).build()));
            item_builder.pub_date(Some(
                match DateTime::parse_from_rfc3339(published_at.as_str()) {
                    Ok(publish_date) => publish_date.to_string(),
                    Err(_) => {
                        warn!(
                            "Using default DateTime due to parsing error. {published_at} could not be parsed"
                        );
                        let default_date: DateTime<FixedOffset> = DateTime::default();
                        default_date.to_string()
                    }
                },
            )); //BUG: possible bug here

            let itunes_item_extension = ITunesItemExtensionBuilder::default()
                .summary(Some(description))
                .duration(Some({
                    let duration_as_string = vod
                        .duration?
                        .replace("h", ":")
                        .replace("m", ":")
                        .replace("s", "");

                    let duration_parts: Vec<&str> = duration_as_string.split(':').collect();

                    match duration_parts.len() {
                        3 => format!(
                            "{:02}:{:02}:{:02}",
                            duration_parts[0], duration_parts[1], duration_parts[2]
                        ),
                        2 => format!("00:{:02}:{:02}", duration_parts[0], duration_parts[1]),
                        1 => format!("00:00:{:02}", duration_parts[0]),
                        _ => {
                            info!(
                                "vod {} has invalid duration {}",
                                video_id, duration_as_string
                            );
                            String::from("00:00:00")
                        }
                    }
                }))
                .image(vod.thumbnail_url)
                .build();
            item_builder.itunes_ext(Some(itunes_item_extension));
            Some(item_builder.build())
        })
        .collect();
    rss_item
}

