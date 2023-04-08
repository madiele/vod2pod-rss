use async_trait::async_trait;
use reqwest::Url;


#[async_trait]
pub trait ConvertableToFeed {
    async fn get_feed_url(&self) -> Url;
}

struct TwitchUrl {
    url: Url,
}

impl From<Url> for TwitchUrl {
    fn from(value: Url) -> Self {
        TwitchUrl { url: value }
    }
}

#[async_trait]
impl ConvertableToFeed for TwitchUrl {
    async fn get_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}


struct YoutubeUrl {
    url: Url,
}

impl From<Url> for YoutubeUrl {
    fn from(value: Url) -> Self {
        YoutubeUrl { url: value }
    }
}

#[async_trait]
impl ConvertableToFeed for YoutubeUrl {
    async fn get_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}

struct GenericUrl {
    url: Url,
}

impl From<Url> for GenericUrl {
    fn from(value: Url) -> Self {
        GenericUrl { url: value }
    }
}

#[async_trait]
impl ConvertableToFeed for GenericUrl {
    async fn get_feed_url(&self) -> Url {
        self.url.to_owned()
    }
}
