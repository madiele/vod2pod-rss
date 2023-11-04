use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset};
use log::{debug, info, warn};
use redis::{ErrorKind, FromRedisValue, RedisError, ToRedisArgs};
use regex::Regex;
use reqwest::Url;
use rss::{
    extension::itunes::{ITunesChannelExtensionBuilder, ITunesItemExtensionBuilder},
    GuidBuilder, ImageBuilder, Item, ItemBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    configs::{conf, Conf, ConfName},
    provider,
};

use super::MediaProvider;

pub struct TwitchProvider {}

#[async_trait]
impl MediaProvider for TwitchProvider {
    fn media_url_regexes(&self) -> Vec<Regex> {
        return vec![regex::Regex::new(r"^https?://(.*\.)?cloudfront\.net/").unwrap()];
    }

    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        info!("trying to convert twitch channel url {}", channel_url);
        let username = channel_url
            .path_segments()
            .ok_or_else(|| eyre::eyre!("Unable to get path segments"))?
            .last()
            .ok_or_else(|| eyre::eyre!("Unable to get last path segment"))?;

        debug!("parsed username {}", username);

        let client_id = &conf().get(ConfName::TwitchClientId)?;
        let client_secret = &conf().get(ConfName::TwitchSecretKey)?;
        debug!("fetching oauth token");
        let oauth_credentials = authorize(client_id, client_secret).await?;

        // add bearer token to request
        let client = reqwest::Client::new();
        let data = client
            .get(format!(
                "https://api.twitch.tv/helix/users?login={}",
                username
            ))
            .bearer_auth(oauth_credentials.oauth_token.clone())
            .header("Client-Id", client_id)
            .send()
            .await?
            .json::<UserData>()
            .await?
            .data;

        let channel = data
            .get(0)
            .ok_or_else(|| eyre::eyre!("No twitch user found"))?;

        debug!("fetched twitch channel: {:?}", channel);

        let vods_data: VodsData = client
            .get(format!(
                "https://api.twitch.tv/helix/videos?user_id={}",
                channel.id
            ))
            .bearer_auth(oauth_credentials.oauth_token)
            .header("Client-Id", client_id)
            .send()
            .await?
            .json()
            .await?;

        let vods = vods_data.data;

        debug!("fetched vods: {:?}", vods);

        let mut channel_builder = provider::build_default_rss_structure();

        let mut image_builder = ImageBuilder::default();
        channel_builder
            .title(channel.display_name.clone())
            .link(channel_url.clone())
            .description(channel.description.clone())
            .itunes_ext(Some(
                ITunesChannelExtensionBuilder::default()
                    .image(Some(channel.profile_image_url.clone()))
                    .author(Some(channel.display_name.clone()))
                    .build(),
            ))
            .image(Some(
                image_builder.url(channel.profile_image_url.clone()).build(),
            ));

        let rss_items = build_items_from_vods(vods);

        Ok(channel_builder.items(rss_items).build().to_string())
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
        TwitchProvider {}
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct User {
    id: String,
    login: String,
    display_name: String,
    #[serde(rename = "type")]
    type_field: String,
    broadcaster_type: String,
    description: String,
    profile_image_url: String,
    offline_image_url: String,
    view_count: i64,
    created_at: String,
}

#[derive(Deserialize, Debug)]
struct UserData {
    data: Vec<User>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct MutedSegments {
    pub duration: i32,
    pub offset: i32,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Video {
    id: String,
    stream_id: Option<String>,
    user_id: String,
    user_login: String,
    user_name: String,
    title: String,
    description: String,
    created_at: String,
    published_at: String,
    url: String,
    thumbnail_url: String,
    viewable: String,
    view_count: i32,
    language: String,
    #[serde(rename = "type")]
    type_field: String,
    duration: String,
    muted_segments: Option<Vec<MutedSegments>>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct VodsData {
    pub data: Vec<Video>,
    pub pagination: serde_json::Value,
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

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OAuthCredentials {
    oauth_token: String,
    oauth_expire_epoch: u64,
}

impl OAuthCredentials {
    fn key() -> &'static str {
        "twitch_oauth_credentials"
    }
}

impl FromRedisValue for OAuthCredentials {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        let raw_json = String::from_redis_value(v)?;
        let credentials: OAuthCredentials = serde_json::from_str(&raw_json).map_err(|_e| {
            RedisError::from((ErrorKind::TypeError, "redis twitch credential corrupted"))
        })?;
        Ok(credentials)
    }
}

impl ToRedisArgs for OAuthCredentials {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let raw_json = serde_json::to_string(self).unwrap();
        raw_json.write_redis_args(out);
    }
}

async fn authorize(client_id: &str, client_secret: &str) -> eyre::Result<OAuthCredentials> {
    let mut redis = crate::get_redis_client().await?;

    let cached_credential: Option<OAuthCredentials> = redis::cmd("GET")
        .arg(OAuthCredentials::key())
        .query_async(&mut redis)
        .await?;

    if let Some(cached_credential) = cached_credential {
        if cached_credential.oauth_expire_epoch
            > SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        {
            info!(
                "using cached twitch oauth credentials, expires in {} seconds",
                cached_credential.oauth_expire_epoch
                    - SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
            );
            return Ok(cached_credential);
        }
    }

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

            let oauth_credentials = OAuthCredentials {
                oauth_token,
                oauth_expire_epoch,
            };

            let _: () = redis::cmd("SET")
                .arg(OAuthCredentials::key())
                .arg(oauth_credentials.clone())
                .query_async(&mut redis)
                .await?;

            info!(
                "got new twitch oauth credentials, expires in {} seconds",
                oauth_credentials.oauth_expire_epoch
                    - SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
            );

            return Ok(oauth_credentials);
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

fn build_items_from_vods(items: Vec<Video>) -> Vec<Item> {
    items.into_iter().map(vod_to_rss_item_converter).collect()
}

fn vod_to_rss_item_converter(vod: Video) -> Item {
    let video_id = vod.id;
    let title = vod.title;
    let description = title.clone();
    let published_at = vod.created_at;
    let mut item_builder = ItemBuilder::default();
    item_builder.title(Some(title.clone()));
    item_builder.description(Some(description.clone()));
    item_builder.link(Some(format!("https://www.twitch.tv/videos/{video_id}")));
    item_builder.guid(Some(GuidBuilder::default().value(video_id.clone()).build()));
    item_builder.pub_date(Some(
        match DateTime::parse_from_rfc3339(published_at.as_str()) {
            Ok(publish_date) => publish_date.to_rfc2822().to_string(),
            Err(_) => {
                warn!(
                    "Using default DateTime due to parsing error. {published_at} could not be parsed"
                );
                let default_date: DateTime<FixedOffset> = DateTime::default();
                default_date.to_rfc2822().to_string()
            }
        },
    ));
    let itunes_item_extension = ITunesItemExtensionBuilder::default()
        .summary(Some(description))
        .duration(Some({
            let duration_as_string = vod
                .duration
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
        .image(Some(
            vod.thumbnail_url
                .replace("%{width}", "512")
                .replace("%{height}", "288"),
        ))
        .build();
    item_builder.itunes_ext(Some(itunes_item_extension));
    item_builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test(tokio::test)]
    async fn fetch_twitch_channel() {
        let provider = TwitchProvider::new();
        conf()
            .get(ConfName::TwitchSecretKey)
            .expect("to run this test set TWITCH_SECRET env var");

        conf()
            .get(ConfName::TwitchClientId)
            .expect("to run this test set TWITCH_CLIENT_ID env var");

        let url = Url::parse("https://www.twitch.tv/tumblurr").unwrap();
        let rss = provider.generate_rss_feed(url).await.unwrap();

        let feed = feed_rs::parser::parse(&rss.into_bytes()[..]).unwrap();

        println!("{:#?}", feed);
        assert!(feed.entries.len() > 1);
        for entry in &feed.entries {
            for media in &entry.media {
                let duration = media.duration.unwrap();
                assert!(duration > std::time::Duration::default());
                assert!(!media.thumbnails[0].image.uri.contains("%{width}"));
                assert!(!media.thumbnails[0].image.uri.contains("%{height}"));
            }
            assert!(entry.title.is_some());
            assert!(entry.summary.is_some());
            assert!(entry.published.is_some());
        }
    }
}

