pub fn conf() -> impl Conf {
    EnvConf {}
}

pub trait Conf {
    fn get(self: &Self, key: ConfName) -> eyre::Result<String>;
}

pub enum ConfName {
    RedisAddress,
    RedisPort,
    RedisUrl,
    Mp3Bitrate,
    TwitchToPodcastUrl,
    YoutubeApiKey,
    TwitchClientId,
    TwitchSecretKey,
    PodTubeUrl,
    TranscodingEnabled,
    SubfolderPath,
    ValidUrlDomains,
}

struct EnvConf { }

impl Conf for EnvConf {
    fn get(self: &Self, key: ConfName) -> eyre::Result<String> {
        match key {
            ConfName::RedisAddress => Ok(std::env::var("REDIS_ADDRESS").unwrap_or_else(|_| "localhost".to_string())),
            ConfName::RedisPort => Ok(std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string())),
            ConfName::RedisUrl => {
                let redis_address = conf().get(ConfName::RedisAddress).unwrap();
                let redis_port = conf().get(ConfName::RedisPort).unwrap();
                Ok(format!("redis://{redis_address}:{redis_port}/"))
            },
            ConfName::Mp3Bitrate => Ok(std::env::var("MP3_BITRATE").unwrap_or_else(|_| "192".to_string())),
            ConfName::TwitchToPodcastUrl => Ok(std::env::var("TWITCH_TO_PODCAST_URL")?),
            ConfName::TwitchClientId => std::env::var("TWITCH_CLIENT_ID")
                .map_err(|e| {eyre::eyre!(e)})
                .and_then(|s| if s.is_empty() { Err(eyre::eyre!("no TwitchClientId api key")) } else { Ok(s) }),
            ConfName::TwitchSecretKey => std::env::var("TWITCH_SECRET")
                .map_err(|e| {eyre::eyre!(e)})
                .and_then(|s| if s.is_empty() { Err(eyre::eyre!("no TwitchSecretKey api key")) } else { Ok(s) }),
            ConfName::YoutubeApiKey => std::env::var("YT_API_KEY")
                .map_err(|e| {eyre::eyre!(e)})
                .and_then(|s| if s.is_empty() { Err(eyre::eyre!("no youtube api key")) } else { Ok(s) }),
            ConfName::PodTubeUrl => Ok(std::env::var("PODTUBE_URL")?),
            ConfName::TranscodingEnabled => Ok(std::env::var("TRANSCODE").unwrap_or_else(|_| "False".to_string())),
            ConfName::SubfolderPath => {
                let mut folder = std::env::var("SUBFOLDER").unwrap_or("".to_string());
                if !folder.starts_with('/') {
                    folder.insert(0, '/');
                }
                while folder.ends_with('/') {
                    folder.pop();
                }
                Ok(folder)
            },
            ConfName::ValidUrlDomains => Ok(std::env::var("VALID_URL_DOMAINS").unwrap_or_else(|_| "https://www.youtube.com/,https://youtube.com/,https://www.youtu.be/,https://www.twitch.tv/,https://twitch.tv/,https://m.twitch.tv/".to_string())),
        }
    }
}
