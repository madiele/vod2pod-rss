pub mod remuxer;

use std::collections::VecDeque;
use std::io::{Write};
use std::str::FromStr;
use std::error::Error;
use std::{io::Read, process::{ Command, Stdio }};

use cached::AsyncRedisCache;
use cached::proc_macro::io_cached;
use futures::Future;
use genawaiter::sync::{ Gen, Co };
use log::{info, debug, error, warn};
use reqwest::Url;
use serde::Serialize;
use tokio::sync::mpsc::{Receiver, Sender, channel};


use crate::configs::{conf, ConfName, Conf, AudioCodec};
use crate::transcoder::remuxer::{EbmlRemuxer, Remuxer};

#[derive(Serialize)]
pub struct FfmpegParameters {
    pub seek_time: f32,
    pub url: Url,
    pub audio_codec: AudioCodec,
    pub bitrate_kbit: usize,
    pub max_rate_kbit: usize,
    pub expected_bytes_count: usize,
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

#[io_cached(
    map_error = r##"|e| eyre::Error::new(e)"##,
    type = "AsyncRedisCache<Url, Url>",
    create = r##" {
        AsyncRedisCache::new("cached_yt_stream_url=", 18000)
            .set_refresh(false)
            .set_connection_string(&conf().get(ConfName::RedisUrl).unwrap())
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

    match output {
        Ok(x) => {
            let raw_url = std::str::from_utf8(&x.stdout).unwrap_or_default();
            match Url::from_str(raw_url) {
                Ok(url) => Ok(url),
                Err(e) => {
                    warn!("error while parsing stream url:\ninput: {}\nerror: {}", raw_url, e.to_string());
                    Err(eyre::eyre!(e))
                },
            }
        }
        Err(e) => Err(eyre::eyre!(e)),
    }
}

impl Transcoder {
    pub async fn new(ffmpeg_paramenters: &FfmpegParameters) -> eyre::Result<Self> {
        let youtube_regex = regex::Regex::new(r#"^(https?://)?(www\.)?(youtu\.be/|youtube\.com/)"#).unwrap();
        let ffmpeg_command = if youtube_regex.is_match(&ffmpeg_paramenters.url.to_string()) {
            info!("detected youtube url, need to find the stream url if not in cache");
            Self::get_ffmpeg_command(&FfmpegParameters {
                seek_time: ffmpeg_paramenters.seek_time,
                url: get_youtube_stream_url(&ffmpeg_paramenters.url).await?,
                audio_codec: ffmpeg_paramenters.audio_codec.to_owned(),
                bitrate_kbit: ffmpeg_paramenters.bitrate_kbit,
                max_rate_kbit: ffmpeg_paramenters.max_rate_kbit,
                expected_bytes_count: ffmpeg_paramenters.expected_bytes_count
            })
        } else {
            Self::get_ffmpeg_command(ffmpeg_paramenters)
        };


        Ok(Self { ffmpeg_command, expected_bytes_count: ffmpeg_paramenters.expected_bytes_count})
    }

    fn get_ffmpeg_command(ffmpeg_paramenters: &FfmpegParameters) -> Command {
        debug!("generating ffmpeg command");
        let mut command = Command::new("ffmpeg");
        let command_ref = &mut command;
        command_ref
            .args(["-ss", ffmpeg_paramenters.seek_time.to_string().as_str()])
            .args(["-i", ffmpeg_paramenters.url.as_str()])
            .args(["-acodec", ffmpeg_paramenters.audio_codec.get_ffmpeg_codec_str()])
            .args(["-vbr", "off"])
            .args(["-ab", format!("{}k", ffmpeg_paramenters.bitrate_kbit).as_str()])
            .args(["-f", ffmpeg_paramenters.audio_codec.get_extension_str()])
            .args(["-bufsize", (ffmpeg_paramenters.bitrate_kbit * 30).to_string().as_str()])
            .args(["-maxrate", format!("{}k", ffmpeg_paramenters.max_rate_kbit).as_str()])
            .args(["-timeout", "300"])
            .args(["-hide_banner"])
            .args(["-loglevel", "error"])
            .args(["pipe:stdout"]);
        let args: Vec<String> = command_ref.get_args().map(|x| {x.to_string_lossy().to_string()}).collect();
        info!(
            "generated ffmpeg command:\n{} {}",
            command_ref.get_program().to_string_lossy(),
            args.join(" ")
        );
        command
    }

    pub fn get_transcode_stream(
        self,
    ) -> Gen<Result<actix_web::web::Bytes, impl Error>, (), impl Future<Output = ()>> {

        async fn generetor_coroutine(
            mut command: Command,
            expected_bytes_count: usize,
            co: Co<Result<actix_web::web::Bytes, std::io::Error>>
        ) {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to run commnad");

            let mut err = child.stderr.take().expect("failed to open stderr");
            let mut out = child.stdout.take().expect("failed to open stdout");

            let channel_size: usize = 10;
            let (tx, mut rx): (Sender<Result<actix_web::web::Bytes, std::io::Error>>,
                                    Receiver<Result<actix_web::web::Bytes, std::io::Error>>) = channel(channel_size);

            let tx_stdout = tx.clone();
            let tx_stderr = tx;

            //stderr thread
            std::thread::spawn(move || {
                loop {
                    let mut buf = String::new();
                    match err.read_to_string(&mut buf) {
                        Ok(0) => {
                            debug!("ffmpeg stderr closed");
                            break;
                        },
                        Ok(_) => {
                            error!("{}", buf);
                            let _ = tx_stderr.blocking_send(Err(std::io::Error::new(std::io::ErrorKind::Other, buf)));
                        },
                        Err(e) => {
                            error!("failed to read from stderr: {}", e);
                            let _ = tx_stderr.blocking_send(Err(std::io::Error::new(std::io::ErrorKind::Other, e)));
                            break;
                        }
                    }
                }
            });

            //stdout thread
            std::thread::spawn(move || {
                const BUFFER_SIZE: usize = 1024*100;
                //let mut tries = 0;
                let mut sent_bytes_count: usize = 0;
                //let mut first_cluster_passed = false;
                //let ffmpeg_out_webm = WebmIterator::new(out, &[]);

                //let (remuxed_in , mut remuxed_out) = channel(1000);
                //std::thread::spawn(move || {
                //    for tag in ffmpeg_out_webm {
                //        if let Ok(tag) = tag {
                //            match tag {
                //                MatroskaSpec::Cluster(Master::Start) => {
                //                    if true {
                //                        _ = remuxed_in.blocking_send(tag);
                //                        continue;
                //                    }
                //                    first_cluster_passed = true;
                //                    _ = remuxed_in.blocking_send(MatroskaSpec::Cues(Master::Start));
                //                    //timestap are in nanoseconds so 1000 == 1 second, testing a 150 seconds clip
                //                    //at 192k each second is 192000/8 bytes
                //                    for i in 0..149 {
                //                        let cluster_position = (i + 1) * (192000/8);
                //                        let cluster_timestamp = (i + 1) * 1000;
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CuePoint(Master::Start));
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CueTime(cluster_timestamp));
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CueTrackPositions(Master::Start));
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CueTrack(1));
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CueClusterPosition(cluster_position));
                //                        _ = remuxed_in.blocking_send(MatroskaSpec::CuePoint(Master::End));
                //                    }
                //                    _ = remuxed_in.blocking_send(MatroskaSpec::Cues(Master::End));
                //                    _ = remuxed_in.blocking_send(MatroskaSpec::Cluster(Master::Start));
                //                },
                //                tag => _ = remuxed_in.blocking_send(tag),
                //            };
                //        } else { break; }
                //    }
                //});

                let mut buff: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                let mut remuxer = EbmlRemuxer::new(VecDeque::new());
                loop {
                    match out.read(&mut buff) {
                        Ok(readed) => {
                            //if read_bytes == 0 { break };
                            //TODO: when seeking to a non 0 point we should probably throw away the header?

                            //TODO: as an experiment try to rewrite the ebml with Cue and overwrite the original buffer (check that the new buffer is < than BUFFER_SIZE) then see if seeking works
                            //TODO: if it works refactor and abstract this so that the MP3 codec is left untouched
                            if sent_bytes_count + readed > expected_bytes_count {
                                //partial request is fulfilled we only need to send the remaining data
                                let bytes_remaining = expected_bytes_count - sent_bytes_count;
                                _ = tx_stdout.blocking_send(Ok(actix_web::web::Bytes::copy_from_slice(&buff[..bytes_remaining])));
                                info!("transcoded everything in partial request");
                                _ = child.kill();
                                _ = child.wait();
                                break;
                            }

                            if readed == 0 {
                                const NULL_BUFF: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                                info!("transcoded everything");
                                _ = remuxer.flush();

                                //FIX: block just for testing, it works! but due to a limitation of can only flush the buffer if the whole file is read
                                {
                                    let collect: actix_web::web::Bytes = remuxer.get_mut().drain(..).collect();
                                    debug!("2: {:?}", collect);
                                    if collect.first().is_none() {continue;}
                                    let send_res = tx_stdout.blocking_send(Ok(collect));
                                    if let Err(e) = send_res {
                                        debug!("{}", e);
                                        info!("connection to client dropped, stopping transcode");
                                        _ = child.kill();
                                        _ = child.wait();
                                        break;
                                    };
                                }

                                //pad end of stream with 00000000 bytes if client expects more data to be sent
                                debug!("sending {} bytes of padding", expected_bytes_count - sent_bytes_count);
                                while sent_bytes_count < expected_bytes_count {
                                    let padding_bytes = expected_bytes_count - sent_bytes_count;
                                    if padding_bytes >= BUFFER_SIZE {
                                        _ = tx_stdout.blocking_send(Ok(actix_web::web::Bytes::copy_from_slice(&NULL_BUFF)));
                                        sent_bytes_count += BUFFER_SIZE;
                                    } else {
                                        _ = tx_stdout.blocking_send(Ok(actix_web::web::Bytes::copy_from_slice(&NULL_BUFF[..padding_bytes.try_into().unwrap_or(0)])));
                                        sent_bytes_count += padding_bytes;
                                    }
                                }
                                _ = child.wait();
                                break;
                            }

                            //debug!("1: {:?}", buff);
                            _ = remuxer.write(&buff);
                            let collect: actix_web::web::Bytes = remuxer.get_mut().drain(..).collect(); //FIX: due to a limitation in TagWriter this is always empty, an incomplete flush is needed
                            debug!("2: {:?}", collect);
                            if collect.first().is_none() {continue;} //TODO: remove just for testing
                            let send_res = tx_stdout.blocking_send(Ok(collect));
                            if let Err(e) = send_res {
                                debug!("{}", e);
                                info!("connection to client dropped, stopping transcode");
                                _ = child.kill();
                                _ = child.wait();
                                break;
                            };


                            sent_bytes_count += readed;
                        }
                        Err(_e) => {
                            panic!();
                        }
                    }
                };
            });



            info!("streaming to client");
            while let Some(x) = rx.recv().await {
                match x {
                    Ok(bytes) => co.yield_(Ok(bytes)).await,
                    Err(e) => { rx.close(); co.yield_(Err(e)).await },
                }
            }
        }
        Gen::new(|co| generetor_coroutine(self.ffmpeg_command, self.expected_bytes_count, co))
    }
}

#[cfg(test)]
mod test {
    use log::info;
    use super::*;

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
                Some("pipe:stdout") => {
                    info!("pipe:stdout");
                }
                Some("-timeout") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    info!("-timeout {}", value);
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
