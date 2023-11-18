use async_trait::async_trait;
use regex::Regex;
use reqwest::Url;
use serde::Deserialize;

use crate::configs::{conf, Conf, ConfName};

use super::MediaProvider;

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct Video {
    streamingPlaylists: Vec<StreamingPlaylist>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct StreamingPlaylist {
    playlistUrl: Url,
}

pub struct PeerTubeProvider;

#[async_trait]
impl MediaProvider for PeerTubeProvider {
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        Ok(reqwest::get(channel_url).await?.text().await?)
    }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        let video_url = find_api_url(media_url).await?;

        let response = reqwest::get(video_url).await?;
        let video: Video = response.json().await?;

        Ok(video.streamingPlaylists[0].playlistUrl.clone())
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        let hosts = get_peertube_hosts();
        let mut regexes: Vec<Regex> = Vec::with_capacity(hosts.len());
        for host in hosts {
            regexes
                .push(Regex::new(&host.to_string().replace('.', "\\.").replace('*', ".+")).unwrap())
        }

        regexes
    }
}

async fn find_api_url(media_url: &Url) -> eyre::Result<Url> {
    let mut video_url = media_url.clone();

    let foud_uuid = video_url
        .path_segments()
        .unwrap()
        .find_map(|x| uuid::Uuid::parse_str(x).ok());

    let uuid = foud_uuid
        .ok_or_else(|| eyre::eyre!("could not find uuid in: {:?}", video_url.to_string()))?;

    video_url
        .path_segments_mut()
        .unwrap()
        .clear()
        .push("api")
        .push("v1")
        .push("videos")
        .push(&uuid.to_string());

    Ok(video_url)
}

fn get_peertube_hosts() -> Vec<String> {
    let binding = conf().get(ConfName::PeerTubeValidHosts).unwrap();
    let patterns: Vec<String> = binding
        .split(',')
        .filter(|e| !e.trim().is_empty())
        .map(|x| x.to_string())
        .collect();
    patterns
}

