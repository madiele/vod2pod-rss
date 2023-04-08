use async_trait::async_trait;
use reqwest::Url;

pub fn from(url: Url) -> Box<dyn ConvertableToFeed> {
    match url {
        matches if url.domain().unwrap_or_default().contains("twitch.tv") => Box::new(TwitchUrl { url }),
        matches if url.domain().unwrap_or_default().contains("youtube.com") || url.domain().unwrap_or_default().contains("youtu.be") => Box::new(YoutubeUrl { url }),
        _ => Box::new(GenericUrl { url }),
    }
}

#[async_trait]
pub trait ConvertableToFeed {
    async fn to_feed_url(&self) -> Url;
}

struct TwitchUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for TwitchUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}


struct YoutubeUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for YoutubeUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}

struct GenericUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for GenericUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}
