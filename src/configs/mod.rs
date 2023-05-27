use log::warn;
use serde::Serialize;

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
    AudioCodec,
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
            ConfName::ValidUrlDomains => Ok(std::env::var("VALID_URL_DOMAINS").unwrap_or_else(|_| "https://*.youtube.com/,https://youtube.com/,https://youtu.be/,https://*.youtu.be/,https://*.twitch.tv/,https://twitch.tv/,https://*.googlevideo.com/,https://*.cloudfront.net/".to_string())),
            ConfName::AudioCodec => Ok(std::env::var("AUDIO_CODEC").map(|c| {
                match c.as_str() {
                    "MP3" => c,
                    "OPUS" => c,
                    "OGG" => "OGG_VORBIS".to_string(),
                    "VORBIS" => "OGG_VORBIS".to_string(),
                    "OGG_VORBIS" => c,
                    _ => {
                        warn!("Unrecognized codec \"{c}\". Defaulting to MP3.");
                        "MP3".to_string()
                    }
                }
            }).unwrap_or_else(|_| "MP3".to_string())),
        }
    }
}

#[derive(Serialize,Clone, Copy)]
pub enum AudioCodec {
    MP3,
    Opus,
    OGGVorbis,
}

impl AudioCodec {
    pub fn get_ffmpeg_codec_str(&self) -> &'static str {
        match self {
            AudioCodec::MP3 => "libmp3lame",
            AudioCodec::Opus => {
                warn!("seeking is not supported with OPUS codec  ");
                "libopus"
            },
            AudioCodec::OGGVorbis => {
                warn!("seeking is not supported with OGG_VORBIS codec ... ");
                "libvorbis"
            },
        }
    }

    pub fn get_extension_str(&self) -> &'static str {
        match self {
            AudioCodec::MP3 => "mp3",
            AudioCodec::Opus => "webm",
            AudioCodec::OGGVorbis => "webm",
        }
    }

    pub fn get_mime_type_str(&self) -> &'static str {
        match self {
            AudioCodec::MP3 => "audio/mpeg",
            AudioCodec::Opus => "audio/webm",
            AudioCodec::OGGVorbis => "audio/webm",
        }
    }
}

impl From<String> for AudioCodec {
    fn from(value: String) -> Self {
        match value.as_str() {
            "MP3" => AudioCodec::MP3,
            "OPUS" => AudioCodec::Opus,
            "OGG_VORBIS" => AudioCodec::OGGVorbis,
            _ => AudioCodec::MP3,
        }
    }
}

impl Default for AudioCodec {
    fn default() -> Self {
        Self::MP3
    }
}
