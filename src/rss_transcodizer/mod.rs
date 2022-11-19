//this module will take an existing RSS and output a new RSS with the enclosure replaced by the trascode URL
//usage example
//GET /rss?url=https://website.com/url/to/feed.rss
//media will have original url replaced by
//GET /transcode/UUID?url=https://website.com/url/to/media.mp3
//GET /transcode/UUID?url=https://website.com/url/to/media.mp4
//GET /transcode/UUID?url=https://website.com/url/to/media.m3u8 (this will require some trickery to get the correct duration)
//in the frist version those calls will use the duration field in the RSS with the constant bitrate of the transcoder to generate
//a correct byte range
//in future version they will try to get the first secs of the original to get the bitrate and original byte range

use std::borrow::BorrowMut;

use eyre::eyre;
use log::warn;
use regex::Regex;
use reqwest::Url;
use rss::{ Channel, Enclosure };
use serde_json::error;
use uuid::Uuid;

pub struct RssTranscodizer {
    url: String,
    transcode_service_url: Url,
}

impl RssTranscodizer {
    pub fn new(url: String, transcode_service_url: Url) -> Self {
        Self { url, transcode_service_url }
    }

    pub async fn transcodize(&self) -> eyre::Result<String> {
        let rss_body = (async { reqwest::get(&self.url).await?.bytes().await }).await?;
        let generation_uuid  = uuid::Uuid::new_v4().to_string();

        //RSS parsing
        let mut rss = match Channel::read_from(&rss_body[..]) {
            Ok(x) => x,
            Err(e) => {
                return Err(eyre!("could not parse rss, reason: {e}"));
            }
        };

        //RSS translation
        for item in &mut rss.items {
            let enclosure = match &item.enclosure {
                Some(x) => x.clone(),
                None => {
                    warn!(
                        "item has no media, skipping: ({})",
                        serde_json::to_string(&item).unwrap_or_default()
                    );
                    continue;
                }
            };

            let media_url = match Url::parse(enclosure.url()) {
                Ok(x) => x,
                Err(_) => {
                    warn!("item has an invalid url");
                    continue;
                }
            };

            let duration_str = item.itunes_ext
                .clone()
                .unwrap_or_default()
                .duration.unwrap_or("".to_string());

            let duration_data = match DurationString::new(&duration_str) {
                Ok(x) => x,
                Err(e) => {
                    warn!("failed to parse duration for {media_url}:\n{e}");
                    continue;
                }
            };

            let duration_secs = duration_data.to_seconds();

            let mut transcode_service_url = self.transcode_service_url.clone();
            let bitarate = 64; //TODO refactor
            transcode_service_url
                .query_pairs_mut()
                .append_pair("url", media_url.as_str())
                .append_pair("bitrate", bitarate.to_string().as_str())
                .append_pair("UUID", generation_uuid.as_str())
                .append_pair("duration", duration_secs.to_string().as_str());

            item.set_enclosure(Enclosure {
                length: (bitarate * 1024 * duration_secs).to_string(),
                url: transcode_service_url.to_string(),
                mime_type: "audio/mpeg".to_string(),
            });
        }

        Ok(rss.to_string())
    }
}

struct DurationString {
    h: u32,
    m: u32,
    s: u32,
}
impl DurationString {
    fn new(duration_unparsed: &String) -> eyre::Result<Self> {
        let re = Regex::new("(?P<h>[0-9]{2,5}):(?P<m>[0-9]{2}):(?P<s>[0-9]{2})").unwrap();
        let matches = match re.captures_iter(duration_unparsed).next() {
            Some(x) => x,
            None => {
                return Err(eyre!("regex failed for string '{duration_unparsed}'"));
            }
        };
        Ok(DurationString {
            h: u32::from_str_radix(&matches["h"], 10)?,
            m: u32::from_str_radix(&matches["m"], 10)?,
            s: u32::from_str_radix(&matches["s"], 10)?,
        })
    }

    fn to_seconds(&self) -> u32 {
        self.h * 60 * 60 + self.m * 60 + self.s
    }
}