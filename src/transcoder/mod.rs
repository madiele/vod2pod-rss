use std::error::Error;
use std::io::Read;
use std::process::Command;
use std::process::Stdio;
use std::thread::sleep;
use std::time::Duration;

use actix_web::web::Bytes;
use futures::Future;
use genawaiter::sync::{Co, Gen};
use log::info;
use log::{debug, error, warn};
use reqwest::Url;
use serde::Serialize;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use crate::configs::AudioCodec;
use crate::provider;
use crate::provider::MediaProvider;

#[derive(Serialize)]
pub struct FfmpegParameters {
    pub seek_time: f32,
    pub url: Url,
    pub audio_codec: AudioCodec,
    pub bitrate_kbit: usize,
    pub max_rate_kbit: usize,
    pub expected_bytes_count: usize,
    pub timeout_in_seconds: usize,
}

impl FfmpegParameters {
    pub fn bitarate(&self) -> usize {
        self.bitrate_kbit * 1024
    }
}

pub struct Transcoder {
    ffmpeg_command: Command,
    expected_bytes_count: usize,
}

impl Transcoder {
    pub async fn new(ffmpeg_paramenters: &FfmpegParameters) -> eyre::Result<Self> {
        let provider = provider::from(&ffmpeg_paramenters.url);

        let ffmpeg_command = Self::get_ffmpeg_command(&FfmpegParameters {
            seek_time: ffmpeg_paramenters.seek_time,
            url: provider.get_stream_url(&ffmpeg_paramenters.url).await?,
            audio_codec: ffmpeg_paramenters.audio_codec.to_owned(),
            bitrate_kbit: ffmpeg_paramenters.bitrate_kbit,
            max_rate_kbit: ffmpeg_paramenters.max_rate_kbit,
            expected_bytes_count: ffmpeg_paramenters.expected_bytes_count,
            timeout_in_seconds: ffmpeg_paramenters.timeout_in_seconds,
        });

        Ok(Self {
            ffmpeg_command,
            expected_bytes_count: ffmpeg_paramenters.expected_bytes_count,
        })
    }

    fn get_ffmpeg_command(ffmpeg_paramenters: &FfmpegParameters) -> Command {
        debug!("generating ffmpeg command");
        let mut command = Command::new("ffmpeg");
        let command_ref = &mut command;

        command_ref
            .args(["-ss", ffmpeg_paramenters.seek_time.to_string().as_str()])
            .args([
                "-protocol_whitelist",
                "file,http,https,tcp,tls",
                "-i",
                ffmpeg_paramenters.url.as_str(),
            ])
            .args([
                "-acodec",
                ffmpeg_paramenters.audio_codec.get_ffmpeg_codec_str(),
            ])
            .args([
                "-ab",
                format!("{}k", ffmpeg_paramenters.bitrate_kbit).as_str(),
            ])
            .args(["-f", ffmpeg_paramenters.audio_codec.get_extension_str()])
            .args([
                "-bufsize",
                (ffmpeg_paramenters.bitrate_kbit * 30).to_string().as_str(),
            ])
            .args([
                "-maxrate",
                format!("{}k", ffmpeg_paramenters.max_rate_kbit).as_str(),
            ])
            .args([
                "-timeout",
                ffmpeg_paramenters.timeout_in_seconds.to_string().as_str(),
            ])
            .args(["-hide_banner"])
            .args(["-loglevel", "error"])
            .arg("-");
        let args: Vec<String> = command_ref
            .get_args()
            .map(|x| x.to_string_lossy().to_string())
            .collect();
        info!(
            "generated ffmpeg command:\n{} {}",
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
            expected_bytes_count: usize,
            co: Co<Result<Bytes, std::io::Error>>,
        ) {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to run commnad");

            let mut err = child.stderr.take().expect("failed to open stderr");
            let mut out = child.stdout.take().expect("failed to open stdout");

            let channel_size: usize = 10;
            type ChannelBytes = Result<Bytes, std::io::Error>;
            let (tx, mut rx): (Sender<ChannelBytes>, Receiver<ChannelBytes>) =
                channel(channel_size);

            let tx_stdout = tx.clone();
            let tx_stderr = tx;

            //stderr thread
            std::thread::spawn(move || loop {
                let mut buf = String::new();
                match err.read_to_string(&mut buf) {
                    Ok(0) => {
                        debug!("ffmpeg stderr closed");
                        break;
                    }
                    Ok(_) => {
                        error!("{}", buf);
                        let _ = tx_stderr.blocking_send(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            buf,
                        )));
                    }
                    Err(e) => {
                        error!("failed to read from stderr: {}", e);
                        let _ = tx_stderr
                            .blocking_send(Err(std::io::Error::new(std::io::ErrorKind::Other, e)));
                        break;
                    }
                }
            });

            //stdout thread
            std::thread::spawn(move || {
                const BUFFER_SIZE: usize = 1024;
                let mut buff: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                let mut tries = 0;
                let mut sent_bytes_count: usize = 0;
                loop {
                    match out.read(&mut buff) {
                        Ok(read_bytes) => {
                            if sent_bytes_count + read_bytes > expected_bytes_count {
                                //partial request is fulfilled we only need to send the remaining data
                                let bytes_remaining = expected_bytes_count - sent_bytes_count;
                                _ = tx_stdout.blocking_send(Ok(Bytes::copy_from_slice(
                                    &buff[..bytes_remaining],
                                )));
                                info!("transcoded everything in partial request");
                                _ = child.kill();
                                _ = child.wait();
                                break;
                            }

                            if read_bytes == 0 {
                                info!("transcoded everything");
                                //pad end of stream with 00000000 bytes if client expects more data to be sent
                                const NULL_BUFF: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                                debug!(
                                    "sending {} bytes of padding",
                                    expected_bytes_count - sent_bytes_count
                                );
                                while sent_bytes_count < expected_bytes_count {
                                    let padding_bytes = expected_bytes_count - sent_bytes_count;
                                    if padding_bytes >= BUFFER_SIZE {
                                        _ = tx_stdout
                                            .blocking_send(Ok(Bytes::copy_from_slice(&NULL_BUFF)));
                                        sent_bytes_count += BUFFER_SIZE;
                                    } else {
                                        _ = tx_stdout.blocking_send(Ok(Bytes::copy_from_slice(
                                            &NULL_BUFF[..padding_bytes],
                                        )));
                                        sent_bytes_count += padding_bytes;
                                    }
                                }
                                _ = child.wait();
                                break;
                            }

                            let send_res = tx_stdout
                                .blocking_send(Ok(Bytes::copy_from_slice(&buff[..read_bytes])));

                            if let Err(e) = send_res {
                                debug!("{}", e);
                                info!("connection to client dropped, stopping transcode");
                                _ = child.kill();
                                _ = child.wait();
                                break;
                            };
                            sent_bytes_count += read_bytes;
                        }
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::Interrupted => {
                                if tries > 10 {
                                    error!("read was interrupted too many times");
                                    if let Err(err) = tx_stdout.blocking_send(Err(e)) {
                                        error!("unexpected error occured:");
                                        error!("{}", err);
                                        _ = child.kill();
                                        _ = child.wait();
                                        break;
                                    };
                                }
                                warn!("read was interrupted, retrying in 1sec");
                                sleep(Duration::from_secs(1));
                                tries += 1;
                            }
                            _ => {
                                if let Err(err) = tx_stdout.blocking_send(Err(e)) {
                                    error!("unexpected error occured:");
                                    error!("{}", err);
                                    _ = child.kill();
                                    _ = child.wait();
                                    break;
                                };
                            }
                        },
                    };
                }
            });

            info!("streaming to client");
            while let Some(x) = rx.recv().await {
                match x {
                    Ok(bytes) => co.yield_(Ok(bytes)).await,
                    Err(e) => {
                        rx.close();
                        co.yield_(Err(e)).await
                    }
                }
            }
        }
        Gen::new(|co| generetor_coroutine(self.ffmpeg_command, self.expected_bytes_count, co))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use log::info;

    #[tokio::test]
    async fn check_ffmpeg_command() {
        let stream_url = Url::parse("http://url.mp3").unwrap();
        let params = FfmpegParameters {
            seek_time: 30.0,
            url: stream_url,
            max_rate_kbit: 64,
            audio_codec: AudioCodec::MP3,
            bitrate_kbit: 3,
            expected_bytes_count: 999,
            timeout_in_seconds: 600,
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
                Some("-protocol_whitelist") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-protocol_whitelist {}", value);
                    assert_eq!(value, "file,http,https,tcp,tls");
                }
                Some("-i") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-i {}", value);
                    assert_eq!(value, params.url.as_str());
                }
                Some("-acodec") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-acodec {}", value);
                    assert_eq!(value, params.audio_codec.get_ffmpeg_codec_str());
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
                Some("-") => {
                    info!("-");
                }
                Some("-timeout") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-timeout {}", value);
                    assert_eq!(value, "600");
                }
                Some("-hide_banner") => {
                    info!("-hide_banner");
                }
                Some("-loglevel") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-loglevel {}", value);
                }
                Some(x) => panic!("ffmpeg run with uknown option: {x}"),
                None => panic!("ffmpeg run with no options"),
            }
        }
    }
}
