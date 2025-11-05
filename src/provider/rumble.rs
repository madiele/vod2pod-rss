use async_trait::async_trait;
use eyre::eyre;
use log::{debug, info, warn};
use regex::Regex;
use reqwest::{Client, Url};
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

/// Internal struct to hold per-video info while we paginate
#[derive(Debug, Clone)]
struct RumbleVideoStub {
    title: String,
    short_description: Option<String>,
    video_url: String,
    duration_str: Option<String>,
    pub_date_rfc2822: Option<String>,
}

#[async_trait]
impl MediaProvider for RumbleProvider {
    /// Generate RSS for a Rumble channel/user URL
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        info!("starting rumble feed generation for {}", channel_url);

        let client = Client::new();

        // ------------ Config: max items (reuse YouTube max results) ------------
        let max_items: usize = conf()
            .get(ConfName::YoutubeMaxResults)
            .unwrap_or_else(|_| "50".to_string())
            .parse()
            .unwrap_or(50);

        let mut feed_builder: ChannelBuilder = provider::build_default_rss_structure();
        feed_builder.link(channel_url.to_string());
        feed_builder.language(Some("en".to_string()));

        let mut current_url = channel_url.clone();
        let mut is_first_page = true;

        let mut chan_title: Option<String> = None;
        let mut chan_desc_meta: Option<String> = None;
        let mut chan_image_url: Option<String> = None;

        let mut video_stubs: Vec<RumbleVideoStub> = Vec::new();

        // ------------ Paginate channel pages synchronously (no awaits while parsing HTML) ------------
        'pages: loop {
            info!("Rumble: fetching page {}", current_url);
            let html = client
                .get(current_url.clone())
                .send()
                .await?
                .text()
                .await?;

            let (page_title, page_desc_meta, page_image, mut page_stubs, next_url) =
                parse_rumble_channel_page(&html, &current_url, is_first_page, max_items - video_stubs.len());

            if is_first_page {
                chan_title = page_title;
                chan_desc_meta = page_desc_meta;
                chan_image_url = page_image;
                is_first_page = false;
            }

            video_stubs.append(&mut page_stubs);

            if video_stubs.len() >= max_items {
                break 'pages;
            }

            let Some(next) = next_url else {
                info!("Rumble: no further pages, stopping pagination");
                break 'pages;
            };

            current_url = next;
        }

        // ------------ Channel metadata (title, description, image) ------------

        // Title
        let final_title = chan_title.unwrap_or_else(|| {
            warn!("Rumble: failed to pull channel title, falling back to URL");
            channel_url.to_string()
        });
        feed_builder.title(final_title);

        // Description: try /about page, then meta from main page, then fallback
        let about_desc = fetch_rumble_about_description(&client, &channel_url).await;
        let final_desc = about_desc
            .or(chan_desc_meta)
            .unwrap_or_else(|| {
                warn!("Rumble: failed to pull channel description, using fallback");
                format!("Rumble channel: {}", channel_url)
            });
        feed_builder.description(final_desc);

        // Image: prefer avatar, fallback to banner (we already extracted chan_image_url)
        if let Some(img_url) = chan_image_url {
            let mut image_builder = ImageBuilder::default();
            image_builder.url(img_url);
            feed_builder.image(Some(image_builder.build()));
        }

        // ------------ Build RSS items, fetching full video descriptions per video ------------
        let mut items = Vec::new();
        for stub in video_stubs {
            let mut item_builder = ItemBuilder::default();

            item_builder.title(Some(stub.title.clone()));
            item_builder.link(Some(stub.video_url.clone()));
            item_builder.guid(Some(
                GuidBuilder::default()
                    .value(stub.video_url.clone())
                    .build(),
            ));

            // Full description from the video page (fallback to short description)
            let full_desc = fetch_rumble_video_description(&client, &stub.video_url).await
                .or_else(|| stub.short_description.clone());

            if let Some(desc) = &full_desc {
                item_builder.description(Some(desc.clone()));
            }

            // Duration + iTunes extension
            let mut itunes_builder = ITunesItemExtensionBuilder::default();
            if let Some(d) = &stub.duration_str {
                itunes_builder.duration(Some(d.clone()));
            }
            // Optional: also set iTunes summary to the description
            if let Some(desc) = &full_desc {
                itunes_builder.summary(Some(desc.clone()));
            }
            let itunes_ext = itunes_builder.build();
            item_builder.itunes_ext(Some(itunes_ext));

            // PubDate
            if let Some(pd) = &stub.pub_date_rfc2822 {
                item_builder.pub_date(Some(pd.clone()));
            }

            // NOTE: enclosure URL is injected later by rss_transcodizer.

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

/// Parse a *single* Rumble channel page (no async here).
/// Returns:
///  - page_title (only on first page)
///  - page_meta_description (only on first page)
///  - page_image_url (only on first page)
///  - stubs for videos on this page (up to remaining_slots)
///  - next page URL (if any)
type RumbleChannelPageParseResult = (
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<RumbleVideoStub>,
    Option<Url>,
);

fn parse_rumble_channel_page(
    html: &str,
    current_url: &Url,
    is_first_page: bool,
    remaining_slots: usize,
) -> RumbleChannelPageParseResult {
    let document = Html::parse_document(html);

    let main_sel = Selector::parse("main").unwrap();
    let Some(main) = document.select(&main_sel).next() else {
        warn!("Rumble: failed to find <main> in rumble page {}", current_url);
        return (None, None, None, Vec::new(), None);
    };

    let mut page_title: Option<String> = None;
    let mut page_desc_meta: Option<String> = None;
    let mut page_image_url: Option<String> = None;

    if is_first_page {
        // Title
        let title_sel = Selector::parse("div.channel-header--title h1").unwrap();
        page_title = main
            .select(&title_sel)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        // Description from meta[name="description"] on the main page
        let meta_desc_sel = Selector::parse(r#"meta[name="description"]"#).unwrap();
        page_desc_meta = document
            .select(&meta_desc_sel)
            .next()
            .and_then(|n| n.value().attr("content"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // Channel image (prefer avatar, fallback to banner)
        let avatar_sel = Selector::parse("img.channel-header--img").unwrap();
        let banner_sel = Selector::parse("img.channel-header--backsplash-img").unwrap();

        page_image_url = main
            .select(&avatar_sel)
            .next()
            .and_then(|n| n.value().attr("src"))
            .map(|s| s.to_string())
            .or_else(|| {
                main.select(&banner_sel)
                    .next()
                    .and_then(|n| n.value().attr("src"))
                    .map(|s| s.to_string())
            });
    }

    // Common selectors for videos
    let grid_sel = Selector::parse("ol.thumbnail__grid div.videostream").unwrap();

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
    let duration_sel = Selector::parse("div.videostream__status--duration").unwrap();

    let videos: Vec<_> = main.select(&grid_sel).collect();
    if videos.is_empty() {
        warn!("Rumble: failed to find video list on {}", current_url);
    }

    let mut stubs: Vec<RumbleVideoStub> = Vec::new();

    for video in videos {
        if stubs.len() >= remaining_slots {
            break;
        }

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
                    let title = t.text().collect::<String>().trim().to_string();
                    info!("Rumble: skipping premium video: {}", title);
                }
                continue;
            }
        }

        // Title
        let title = video
            .select(&title_sel)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| {
                warn!("Rumble: failed to pull video title on {}", current_url);
                "N/A".to_string()
            });

        // Short description from listing
        let short_description = video
            .select(&desc_sel)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        // Link to the video page
        let href = match video
            .select(&link_sel)
            .next()
            .and_then(|n| n.value().attr("href"))
        {
            Some(h) => h,
            None => {
                warn!("Rumble: failed to pull URL to video on {}", current_url);
                continue;
            }
        };

        let video_url = match Url::parse("https://rumble.com")
            .and_then(|base| base.join(href))
        {
            Ok(u) => u.to_string(),
            Err(e) => {
                warn!(
                    "Rumble: error joining video href '{}' on {}: {}",
                    href, current_url, e
                );
                continue;
            }
        };

        // Duration
        let duration_str = video
            .select(&duration_sel)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        // PubDate
        let pub_date_rfc2822 = video
            .select(&time_sel)
            .next()
            .and_then(|n| n.value().attr("datetime"))
            .and_then(|dt| DateTime::parse_from_rfc3339(dt).ok())
            .map(|dt| dt.with_timezone(&Utc).to_rfc2822());

        stubs.push(RumbleVideoStub {
            title,
            short_description,
            video_url,
            duration_str,
            pub_date_rfc2822,
        });
    }

    // Pagination "next" link (may need tweaking if Rumble changes)
    let next_sel =
        Selector::parse("a[rel=\"next\"], a.pagination__next, a.page-link[rel=\"next\"]").unwrap();

    let next_url = document
        .select(&next_sel)
        .next()
        .and_then(|n| n.value().attr("href"))
        .and_then(|href| current_url.join(href).ok());

    (page_title, page_desc_meta, page_image_url, stubs, next_url)
}

/// Build an `/about` URL for a channel, e.g.:
///   https://rumble.com/c/nickjfuentes   -> https://rumble.com/c/nickjfuentes/about
fn build_rumble_about_url(channel_url: &Url) -> Option<Url> {
    let mut s = channel_url.to_string();
    if !s.ends_with('/') {
        s.push('/');
    }
    s.push_str("about");
    Url::parse(&s).ok()
}

/// Try to get a nicer channel description from the `/about` page
async fn fetch_rumble_about_description(client: &Client, channel_url: &Url) -> Option<String> {
    let about_url = build_rumble_about_url(channel_url)?;
    info!("Rumble: fetching about page {}", about_url);

    let html = client.get(about_url).send().await.ok()?.text().await.ok()?;
    let document = Html::parse_document(&html);

    // First try meta[name="description"]
    let meta_sel = Selector::parse(r#"meta[name="description"]"#).ok()?;
    if let Some(meta) = document.select(&meta_sel).next() {
        if let Some(content) = meta.value().attr("content") {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    // Fallback: any obvious "about" / description container if meta is missing
    let about_sel = Selector::parse(
        r#"div.channel-about__description, div.channel-header--about, div.media-description"#,
    )
    .ok()?;
    if let Some(div) = document.select(&about_sel).next() {
        let text = div.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

/// Fetch full video description from the video page
async fn fetch_rumble_video_description(client: &Client, video_url: &str) -> Option<String> {
    info!("Rumble: fetching full description for {}", video_url);
    let html = client.get(video_url).send().await.ok()?.text().await.ok()?;
    let document = Html::parse_document(&html);

    // Your provided block:
    // <div class="container content media-description media-description--expanded"
    //      data-js="media_long_description_container">
    //
    // So we target the data-js first, with a fallback to the class.
    let container_sel = Selector::parse(
        r#"div[data-js="media_long_description_container"], div.media-description--expanded"#,
    )
    .ok()?;

    let container = document.select(&container_sel).next()?;
    let text = container.text().collect::<String>().trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
