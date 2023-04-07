use std::process::Child;
use std::process::ExitStatus;
use std::str::FromStr;
use std::{ error::Error, time::Duration };
use std::{io::Read, process::{ Command, Stdio }};

use cached::AsyncRedisCache;
use actix_rt::time::sleep;
use actix_web::web::Bytes;
use cached::proc_macro::io_cached;
use futures::Future;
use genawaiter::sync::{ Gen, Co };
use log::{ debug, error, warn };
use reqwest::Url;
use serde::Serialize;

//const PID_TABLE: &str = "active_transcodes";
static mut RUNNING_CHILDS: Vec<Child> = Vec::new();


#[derive(Serialize)]
pub enum FFMPEGAudioCodec {
    Libmp3lame,
}

impl FFMPEGAudioCodec {
    fn as_str(&self) -> &'static str {
        match self {
            FFMPEGAudioCodec::Libmp3lame => "libmp3lame",
        }
    }
}

impl Default for FFMPEGAudioCodec {
    fn default() -> Self {
        Self::Libmp3lame
    }
}

#[derive(Serialize)]
pub struct FfmpegParameters {
    pub seek_time: u32,
    pub url: Url,
    pub audio_codec: FFMPEGAudioCodec,
    pub bitrate_kbit: u32,
    pub max_rate_kbit: u32,
}

impl FfmpegParameters {
    pub fn bitarate(&self) -> u64 {
        u64::from(self.bitrate_kbit * 1024)
    }
}

pub struct Transcoder {
    ffmpeg_command: Command,
}

#[io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, Url>",
    create = r##" {
        let redis_address = std::env::var("REDIS_ADDRESS").unwrap_or_else(|_| "localhost".to_string());
        let redis_port = std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());

        AsyncRedisCache::new("cached_yt_stream_url=", 18000)
            .set_refresh(false)
            .set_connection_string(&format!("redis://{}:{}/", redis_address, redis_port))
            .build()
            .await
            .expect("get_youtube_stream_url cache")
} "##
)]
async fn get_youtube_stream_url(url: &Url) -> eyre::Result<Url> {
    debug!("getting stream_url for yt video: {}", url);
    let output = tokio::process::Command::new("yt-dlp")
        .arg("-x")
        .arg("--get-url")
        .arg(url.as_str())
        .output().await;

    if let Ok(x) = output {
        let raw_url = std::str::from_utf8(&x.stdout).unwrap();
        Ok(Url::from_str(raw_url).unwrap())
    } else {
        Err(eyre::eyre!("could not get youtube stream url"))
    }

}

impl Transcoder {
    pub async fn new(ffmpeg_paramenters: &FfmpegParameters) -> eyre::Result<Self> {
        let youtube_regex = regex::Regex::new(r#"^(https?://)?(www\.)?(youtu\.be/|youtube\.com/)"#).unwrap();
        let ffmpeg_command = if youtube_regex.is_match(&ffmpeg_paramenters.url.to_string()) {
            Self::get_ffmpeg_command(&FfmpegParameters {
                seek_time: ffmpeg_paramenters.seek_time,
                url: get_youtube_stream_url(&ffmpeg_paramenters.url).await?,
                audio_codec: FFMPEGAudioCodec::Libmp3lame,
                bitrate_kbit: ffmpeg_paramenters.bitrate_kbit,
                max_rate_kbit: ffmpeg_paramenters.max_rate_kbit })
        } else {
            Self::get_ffmpeg_command(ffmpeg_paramenters)
        };


        Ok(Self { ffmpeg_command })
    }

    fn get_ffmpeg_command(ffmpeg_paramenters: &FfmpegParameters) -> Command {
        let mut command = Command::new("ffmpeg");
        let command_ref = &mut command;
        command_ref
            .args(["-ss", ffmpeg_paramenters.seek_time.to_string().as_str()])
            .args(["-i", ffmpeg_paramenters.url.as_str()])
            .args(["-acodec", ffmpeg_paramenters.audio_codec.as_str()])
            .args(["-ab", format!("{}k", ffmpeg_paramenters.bitrate_kbit).as_str()])
            .args(["-f", "mp3"])
            .args(["-bufsize", (ffmpeg_paramenters.bitrate_kbit * 30).to_string().as_str()])
            .args(["-maxrate", format!("{}k", ffmpeg_paramenters.max_rate_kbit).as_str()])
            .args(["-timeout", "300"])
            .args(["pipe:stdout"]);
        let args: Vec<String> = command_ref.get_args().map(|x| {x.to_string_lossy().to_string()}).collect();
        debug!(
            "running ffmpeg with command:\n{} {}",
            command_ref.get_program().to_string_lossy(),
            args.join(" ")
        );
        command
    }

    pub fn get_transcode_stream(
        self,
    ) -> Gen<Result<Bytes, impl Error>, (), impl Future<Output = ()>> {

        async fn generetor_coroutine(
            mut command: Command,
            co: Co<Result<Bytes, std::io::Error>>
        ) {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to run commnad");

            let mut out = child.stdout.take().expect("failed to open stdout");

            //reap all zombie childs
            unsafe {
                let _statuses: Vec<ExitStatus> = RUNNING_CHILDS.iter_mut().filter_map(|child: &mut Child| {
                    if let Ok(Some(status)) = child.try_wait() {
                        RUNNING_CHILDS.retain(|x| x.id() != child.id());
                        debug!("child {} reaped with {status}", child.id());
                        Some(status)
                    } else {
                        None
                    }
                }).collect();
                RUNNING_CHILDS.push(child);
            }

            let mut total_bytes_read: usize = 0;
            let mut buff: [u8; 1024] = [0; 1024];
            let mut tries = 0;
            loop {
                match out.read(&mut buff) {
                    Ok(read_bytes) => {

                        if read_bytes == 0 {
                            debug!("reached EOF; read {total_bytes_read} bytes");
                            break;
                        }
                        total_bytes_read += read_bytes;
                        co.yield_(Ok(Bytes::copy_from_slice(&buff[..read_bytes]))).await;
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::Interrupted => {
                                if tries > 10 {
                                    error!("read was interrupted too many times");
                                    co.yield_(Err(e)).await;
                                }
                                warn!("read was interrupted, retrying in 1sec");
                                sleep(Duration::from_secs(1)).await;
                                tries += 1;
                            }
                            _ => {
                                co.yield_(Err(e)).await
                            },
                        }
                    }
                }
            }
        }
        Gen::new(|co| generetor_coroutine(self.ffmpeg_command, co))
    }


}

#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use log::info;
    use regex::Regex;
    use super::*;

    #[tokio::test]
    async fn check_ffmpeg_command() {
        let stream_url = Url::parse("http://url.mp3").unwrap();
        let params = FfmpegParameters {
            seek_time: 30,
            url: stream_url,
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(&params).await.unwrap();
        let ppath = transcoder.ffmpeg_command.get_program();
        if let Some(x) = ppath.to_str() {
            info!("{} ", x);
            assert_eq!(x, "ffmpeg");
        }
        let mut args = transcoder.ffmpeg_command.get_args();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("-ss") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-ss {}", value);
                    assert_eq!(value, params.seek_time.to_string().as_str());
                }
                Some("-i") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-i {}", value);
                    assert_eq!(value, params.url.as_str());
                }
                Some("-acodec") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-acodec {}", value);
                    assert_eq!(value, params.audio_codec.as_str());
                }
                Some("-ab") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-ab {}", value);
                }
                Some("-f") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-f {}", value);
                    assert_eq!(value, "mp3");
                }
                Some("-bufsize") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-bufsize {}", value);
                }
                Some("-maxrate") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-maxrate {}", value);
                }
                Some("pipe:stdout") => {
                    info!("pipe:stdout");
                }
                Some("-timeout") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-timeout {}", value);
                }
                Some(x) => panic!("ffmpeg run with uknown option: {x}"),
                None => panic!("ffmpeg run with no options"),
            }
        }
    }

    #[tokio::test]
    async fn test_transcoding_generator() {
        let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path_to_mp3.push("src/transcoder/test.mp3");
        let protocol = "file://";
        let path = path_to_mp3.as_os_str().to_str().unwrap();
        let stream_url = Url::parse(&format!("{protocol}{path}")).unwrap();
        info!("{stream_url}");
        let params = FfmpegParameters {
            seek_time: 25,
            url: stream_url,
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(&params).await.unwrap();
        let mut gen = transcoder.get_transcode_stream();
        let mut read_counts = 0;
        info!("!!printing buffer!!");
        loop {
            let state = gen.resume();
            match state {
                genawaiter::GeneratorState::Yielded(x) => {
                    let buff = x.unwrap();

                    let out = &buff;
                    let out = String::from_utf8_lossy(out);
                    print!("{out}");
                    read_counts += 1;
                }
                genawaiter::GeneratorState::Complete(_) => {
                    break;
                }
            }
        }
        assert!(read_counts > 0);
    }
}
