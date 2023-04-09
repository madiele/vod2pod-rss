use async_trait::async_trait;
use reqwest::Url;

pub fn from(url: Url) -> Box<dyn ConvertableToFeed> {
    let url = url.to_owned();
    match url {
        _ if url.domain().unwrap_or_default().contains("twitch.tv") => Box::new(TwitchUrl { url }),
        _ if url.domain().unwrap_or_default().contains("youtube.com") || url.domain().unwrap_or_default().contains("youtu.be") => Box::new(YoutubeUrl { url }),
        _ => Box::new(GenericUrl { url }),
    }
}

#[async_trait]
pub trait ConvertableToFeed {
    async fn to_feed_url(&self) -> Url;
    fn is(&self) -> &str; //usefull for testing
}

struct TwitchUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for TwitchUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }

    fn is(&self) -> &str { "TwitchUrl" }
}


struct YoutubeUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for YoutubeUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }
    fn is(&self) -> &str { "YoutubeUrl" }
}

struct GenericUrl {
    url: Url,
}

#[async_trait]
impl ConvertableToFeed for GenericUrl {
    async fn to_feed_url(&self) -> Url {
        self.url.to_owned()
    }

    fn is(&self) -> &str { "GenericUrl" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_twitch_url_conversion() {
        let url = Url::parse("https://www.twitch.tv/example").unwrap();
        let convertable = from(url);
        assert!(convertable.is() == "TwitchUrl");
    }

    #[test]
    fn test_youtube_url_conversion() {
        let url = Url::parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        let convertable = from(url);
        assert!(convertable.is() == "YoutubeUrl");

        let url = Url::parse("https://youtu.be/dQw4w9WgXcQ").unwrap();
        let convertable = from(url);
        assert!(convertable.is() == "YoutubeUrl");
    }

    #[test]
    fn test_generic_url_conversion() {
        let url = Url::parse("https://example.com").unwrap();
        let convertable = from(url);
        assert!(convertable.is() == "GenericUrl");
    }
}
