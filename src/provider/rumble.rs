use async_trait::async_trait;
use eyre::eyre;
use log::{info, debug, warn};
use regex::Regex;
use reqwest::Url;
use rss::{
    extension::itunes::ITunesItemExtensionBuilder,
    ChannelBuilder, GuidBuilder, ImageBuilder, ItemBuilder,
};
use scraper::{Html, Selector};
use chrono::{DateTime, Utc};
use tokio::process::Command;

use crate::provider;
use crate::configs::{conf, Conf, ConfName};
use super::MediaProvider;

pub struct RumbleProvider;

#[async_trait]
impl MediaProvider for RumbleProvider {
    /// Generate RSS for a Rumble channel/user URL
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        info!("starting rumble feed generation for {}", channel_url);

        let html = reqwest::get(channel_url.as_str()).await?.text().await?;
        let document = Html::parse_document(&html);

        // We mirror PodTube: work inside <main> to avoid header/footer noise
        let main_sel = Selector::parse("main").unwrap();
        let main = document
            .select(&main_sel)
            .next()
            .ok_or_else(|| eyre!("failed to find <main> in rumble page"))?;

        let mut feed_builder = provider::build_default_rss_structure();

        // ----- Channel metadata -----
        let title_sel = Selector::parse("div.channel-header--title h1").unwrap();
        let chan_title = main
            .select(&title_sel)
            .next()
            .and_then(|n| Some(n.text().collect::<String>().trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                warn!("Rumble: failed to pull channel title, falling back to URL");
                channel_url.to_string()
            });

        feed_builder.title(chan_title);

        let thumb_sel = Selector::parse("img.channel-header--img").unwrap();
        if let Some(img) = main.select(&thumb_sel).next() {
            if let Some(src) = img.value().attr("src") {
                let mut image_builder = ImageBuilder::default();
                image_builder.url(src.to_string());
                feed_builder.image(Some(image_builder.build()));
            }
        }

        feed_builder.description("--");
        feed_builder.link(channel_url.to_string());
        feed_builder.language(Some("en".to_string()));

        // ----- Video list -----
        let grid_sel = Selector::parse("ol.thumbnail__grid div.videostream").unwrap();
        let videos: Vec<_> = main.select(&grid_sel).collect();
        if videos.is_empty() {
            warn!("Rumble: failed to find video list");
        }

        let live_sel = Selector::parse(
            "span.video-item--live,\
             span.video-item--upcoming,\
             div.videostream__status--live,\
             div.videostream__footer--live",
        )
        .unwrap();

        let premium_sel = Selector::parse("span.text-link-green").unwrap();
        let title_sel = Selector::parse("h3.thumbnail__title").unwrap();
        let desc_sel = Selector::parse("div.videostream__description").unwrap();
        let link_sel = Selector::parse("a.videostream__link").unwrap();
        let time_sel = Selector::parse("time.videostream__time").unwrap();
        let duration_sel =
            Selector::parse("div.videostream__status--duration").unwrap();

        let mut items = Vec::new();

        for video in videos {
            // Skip live / upcoming
            if video.select(&live_sel).next().is_some() {
                if let Some(t) = video.select(&title_sel).next() {
                    let title = t.text().collect::<String>().trim().to_string();
                    info!("Rumble: skipping live/upcoming video: {}", title);
                }
                continue;
            }

            // Skip premium-only
            if let Some(label) = video.select(&premium_sel).next() {
                if label.text().collect::<String>().trim() == "Premium only" {
                    if let Some(t) = video.select(&title_sel).next() {
                        let title =
                            t.text().collect::<String>().trim().to_string();
                        info!("Rumble: skipping premium video: {}", title);
                    }
                    continue;
                }
            }

            let mut item_builder = ItemBuilder::default();

            // Title
            let title = video
                .select(&title_sel)
                .next()
                .map(|n| n.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| {
                    warn!("Rumble: failed to pull video title");
                    "N/A".to_string()
                });
            item_builder.title(Some(title.clone()));

            // Description
            let description = video
                .select(&desc_sel)
                .next()
                .map(|n| n.text().collect::<String>().trim().to_string());
            if let Some(desc) = &description {
                item_builder.description(Some(desc.clone()));
            }

            // Link to the video page
            let href = video
                .select(&link_sel)
                .next()
                .and_then(|n| n.value().attr("href"))
                .ok_or_else(|| eyre!("Rumble: failed to pull URL to video"))?;

            let video_url =
                Url::parse("https://rumble.com")?.join(href)?.to_string();
            item_builder.link(Some(video_url.clone()));
            item_builder.guid(Some(
                GuidBuilder::default().value(video_url.clone()).build(),
            ));

            // Duration (itunes)
            let duration_str = video
                .select(&duration_sel)
                .next()
                .map(|n| n.text().collect::<String>().trim().to_string());

            // PubDate
            let pub_date = video
                .select(&time_sel)
                .next()
                .and_then(|n| n.value().attr("datetime"))
                .and_then(|dt| DateTime::parse_from_rfc3339(dt).ok())
                .map(|dt| dt.with_timezone(&Utc).to_rfc2822());

            // iTunes extension
            let mut itunes_builder = ITunesItemExtensionBuilder::default();
            if let Some(d) = &duration_str {
                itunes_builder.duration(Some(d.clone()));
            }
            let itunes_ext = itunes_builder.build();
            item_builder.itunes_ext(Some(itunes_ext));

            if let Some(pd) = pub_date {
                item_builder.pub_date(Some(pd));
            }

            // NOTE: enclosure URL will be injected by rss_transcodizer
            // so we do NOT set it here; this matches what Youtube/Twitch flows do.

            items.push(item_builder.build());
        }

        feed_builder.items(items);

        Ok(feed_builder.build().to_string())
    }

    /// Given a Rumble video page URL, return a direct media URL using yt-dlp
    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        debug!("getting stream_url for rumble video: {}", media_url);

        let output = Command::new("yt-dlp")
            .arg("-f")
            .arg("bestaudio/best")
            .arg("--get-url")
            .arg(media_url.as_str())
            .output()
            .await;

        match output {
            Ok(x) => {
                let raw = std::str::from_utf8(&x.stdout)
                    .unwrap_or_default()
                    .trim();
                match Url::parse(raw) {
                    Ok(url) => Ok(url),
                    Err(e) => {
                        warn!(
                            "Rumble: error parsing stream url using yt-dlp:\nerror: {}\nyt-dlp stdout: {}\nyt-dlp stderr: {}",
                            e,
                            raw,
                            std::str::from_utf8(&x.stderr).unwrap_or_default()
                        );
                        Err(eyre!(e))
                    }
                }
            }
            Err(e) => Err(eyre!(e)),
        }
    }

    /// Regexes for Rumble and its media hosts
fn domain_whitelist_regexes(&self) -> Vec<Regex> {
    vec![
        // Any https://rumble.com... URL
        Regex::new(r"^https?://(www\.)?rumble\.com").unwrap(),
        // Rumble’s media hosts
        Regex::new(r"^https?://sp\.rmbl\.ws").unwrap(),
        Regex::new(r"^https?://rmbl\.ws").unwrap(),
    ]
}
}
