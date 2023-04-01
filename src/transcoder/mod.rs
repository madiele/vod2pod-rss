use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{ error::Error, time::Duration };
use std::{io::Read, process::{ Command, Stdio }};

use actix::Addr;
use actix_redis::RespValue::{BulkString, Array};
use actix_redis::{RedisActor, resp_array};
use actix_rt::time::sleep;
use actix_web::web::{ Bytes, Data };
use futures::Future;
use genawaiter::sync::{ Gen, Co };
use log::{ debug, error, warn };
use reqwest::Url;
use serde::Serialize;

const PID_TABLE: &str = "active_transcodes";


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

fn get_youtube_stream_url(url: &Url) -> Option<Url> {
    debug!("getting stream_url for yt video: {}", url);
    let output = Command::new("yt-dlp")
        .arg("-x")
        .arg("--get-url")
        .arg(url.as_str())
        .output();

    if let Ok(x) = output {
        let raw_url = std::str::from_utf8(&x.stdout).unwrap();
        Some(Url::from_str(raw_url).unwrap())
    } else {
        error!("could not get youtube stream url");
        None
    }

}

impl Transcoder {
    pub fn new(ffmpeg_paramenters: &FfmpegParameters) -> Self {
        let youtube_regex = regex::Regex::new(r#"^(https?://)?(www\.)?(youtu\.be/|youtube\.com/)"#).unwrap();
        let ffmpeg_command = if youtube_regex.is_match(&ffmpeg_paramenters.url.to_string()) {
            Self::get_ffmpeg_command(&FfmpegParameters {
                seek_time: ffmpeg_paramenters.seek_time,
                url: get_youtube_stream_url(&ffmpeg_paramenters.url).unwrap(),
                audio_codec: FFMPEGAudioCodec::Libmp3lame,
                bitrate_kbit: ffmpeg_paramenters.bitrate_kbit,
                max_rate_kbit: ffmpeg_paramenters.max_rate_kbit })
        } else {
            Self::get_ffmpeg_command(ffmpeg_paramenters)
        };


        Self {
            ffmpeg_command,
        }
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

    //TODO: pipe the output to ffmpeg again as this outputs variable bitrate anyway
    fn get_youtube_dl_command(youtube_dl_parameters: &FfmpegParameters) -> Command {
        let mut command = Command::new("yt-dlp");

        let command_ref = &mut command;
        let url_with_timestamp = format!("{}&t={}s",
            youtube_dl_parameters.url.as_str(),
            youtube_dl_parameters.seek_time.to_string().as_str());
        command_ref
            .args(["-x"])
            .args(["--audio-format", "mp3"])
            .args(["--audio-quality", format!("{}k", youtube_dl_parameters.bitrate_kbit).as_str()])
            .args(["--extract-audio"])
            .args(["-f", "ba"])
            .args(["--output", "-"])
            .arg(url_with_timestamp)
            .stdout(Stdio::piped());
        let args: Vec<String> = command_ref.get_args().map(|x| {x.to_string_lossy().to_string()}).collect();
        debug!(
            "running yt-dlp with command:\n{} {}",
            command_ref.get_program().to_string_lossy(),
            args.join(" ")
        );
        command
    }


    pub fn get_transcode_stream(
        self,
        redis: Data<Addr<RedisActor>>
    ) -> Gen<Result<Bytes, impl Error>, (), impl Future<Output = ()>> {
        async fn generetor_coroutine(
            redis: Data<Addr<RedisActor>>,
            mut command: Command,
            co: Co<Result<Bytes, std::io::Error>>
        ) {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to run commnad");
            let pid =  child.id().to_string();
            _ = redis.send(actix_redis::Command(resp_array!["HSET", PID_TABLE, &pid, get_epoch()])).await;
            kill_stalled_transcodes(&redis).await;

            let mut out = child.stdout.take().expect("failed to open stdout");
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
                        _ = redis.send(actix_redis::Command(resp_array!["HSET", PID_TABLE, &pid, get_epoch()])).await;
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
                            _ => co.yield_(Err(e)).await,
                        }
                    }
                }
            }
        }
        Gen::new(|co| generetor_coroutine(redis, self.ffmpeg_command, co))
    }


}

async fn kill_stalled_transcodes(redis: &Data<Addr<RedisActor>>) {
    if let Ok(Ok(Array(pids))) = redis.send(actix_redis::Command(resp_array!["HGETALL", PID_TABLE])).await {
        let transcode_keys = pids.iter().filter_map(|v| {
            if let BulkString(item) = v {
                return Some(String::from_utf8(item.to_vec()).unwrap())
            } else {
                return None
            }
        }).collect::<Vec<String>>();

        debug!("running transcodes: {:?}", &transcode_keys.join(", "));

        for key in transcode_keys {
            if let Ok(Ok(BulkString(res))) = redis.send(actix_redis::Command(resp_array!["HGET", PID_TABLE, &key])).await {
                let epoch_string = String::from_utf8(res.to_vec()).unwrap();

                let epoch_last_transcode_activity: u32 = epoch_string.trim().parse().unwrap();
                let epoch_now: u32 = get_epoch().parse().unwrap();

                if epoch_now - epoch_last_transcode_activity > 600 {
                    let pid_str = key.trim_start_matches("pid__");
                    match std::process::Command::new("kill")
                        .arg("-9")
                        .arg(format!("{}", pid_str))
                        .spawn() {
                            Ok(_) => {
                                _ = redis.send(actix_redis::Command(resp_array!["HDEL", PID_TABLE, pid_str])).await;
                                debug!("Transcode with PID {} killed successfully", pid_str);
                            },
                            Err(e) => debug!("Error killing process with PID {}: {:?}", pid_str, e),
                    }
                }
            }
        }
    }
}

fn get_epoch() -> String { format!("{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()) }

#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use log::info;
    use regex::Regex;
    use super::*;

    #[test]
    fn check_ffmpeg_command() {
        let duration = 60;
        let stream_url = Url::parse("http://url.mp3").unwrap();
        let params = FfmpegParameters {
            seek_time: 30,
            url: stream_url,
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(&params);
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
                Some(x) => panic!("ffmpeg run with uknown option: {x}"),
                None => panic!("ffmpeg run with no options"),
            }
        }
    }

    #[test]
    fn check_ffmpeg_runs() {
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
        let mut transcoder = Transcoder::new(&params);
        match transcoder.ffmpeg_command.output() {
            Ok(x) => {
                let out = x.stderr.as_slice();
                let out = String::from_utf8_lossy(out);
                info!("command run sucessfully \nOutput: {out}");
                let regex = Regex::new("time=00:00:02.19").unwrap();
                assert!(regex.is_match(out.to_string().as_str()));
            }
            Err(e) => panic!("ffmpeg error {e}"),
        }
    }

    #[test]
    fn test_transcoding_generator() {
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
        let transcoder = Transcoder::new(&params);
        let redis = RedisActor::start("localhost");
        let mut gen = transcoder.get_transcode_stream(Data::new(redis));
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
