use std::{ process::{ Command, Stdio }, io::{ Read }, error::Error };

use actix_web::{ dev::{ ServiceRequest, ServiceResponse }, HttpResponse, web::Bytes };
use futures::Future;
use genawaiter::{ yield_, sync::{ Gen, Co }, sync_producer };

enum FFMPEGAudioCodec {
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

pub struct FFMPEG_parameters {
    seek_time: u32,
    url: String,
    audio_codec: FFMPEGAudioCodec,
    bitrate_kbit: u32,
    max_rate_kbit: u32,
}

impl FFMPEG_parameters {
    pub fn bitarate(&self) -> u64 {
        u64::from(self.bitrate_kbit * 1024)
    }
}

pub struct Streaming_settings {}

pub struct Transcoder<'a> {
    duratiion_s: i32,
    stream_url: &'a str,
    ffmpeg_command: Command,
}

impl<'a> Transcoder<'a> {
    pub fn new(
        duratiion_s: i32,
        stream_url: &'a str,
        ffmpeg_paramenters: &FFMPEG_parameters
    ) -> Self {
        Self {
            duratiion_s,
            stream_url,
            ffmpeg_command: Self::get_ffmpeg_command(ffmpeg_paramenters),
        }
    }

    fn get_ffmpeg_command(ffmpeg_paramenters: &FFMPEG_parameters) -> Command {
        let mut command = Command::new("ffmpeg");
        let command_ref = &mut command;
        command_ref
            .args(["-ss", ffmpeg_paramenters.seek_time.to_string().as_str()])
            .args(["-i", ffmpeg_paramenters.url.as_str()])
            .args(["-acodec", ffmpeg_paramenters.audio_codec.as_str()])
            .args(["-ab", format!("{}k", ffmpeg_paramenters.bitrate_kbit / 1000).as_str()])
            .args(["-f", "mp3"])
            .args(["-bufsize", (ffmpeg_paramenters.bitrate_kbit * 30).to_string().as_str()])
            .args(["-maxrate", format!("{}k", ffmpeg_paramenters.max_rate_kbit).as_str()])
            .args(["pipe:stdout"]);
        command
    }

    pub fn duratiion_s(&self) -> i32 {
        self.duratiion_s
    }

    pub fn stream_url(&self) -> &str {
        self.stream_url
    }

    //TODO: should not be here, move in another module
    async fn start_streaming_to_client(
        self
    ) -> HttpResponse {
        let stream = self.get_transcode_stream();
        HttpResponse::Ok().content_type("audio/mpeg").streaming(stream)
    }

    fn get_transcode_stream(self) -> Gen<Result<Bytes, impl Error>, (), impl Future<Output = ()>> {
        async fn generetor_coroutine(mut command: Command, co: Co<Result<Bytes, std::io::Error>>) {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to run commnad");

            let mut out = child.stdout.take().expect("failed to open stdout");
            loop {
                let mut buff: [u8; 1024] = [0; 1024];
                let res = out.read(&mut buff);
                match res {
                    Ok(read_bytes) => {
                        if read_bytes == 0 {
                            break;
                        }
                        co.yield_(Ok(Bytes::copy_from_slice(&buff))).await;
                    }
                    Err(e) => co.yield_(Err(e)).await,
                }
            }
        }
        Gen::new(|co| generetor_coroutine(self.ffmpeg_command, co))
    }

    fn total_byte_len(&self, bitrate: i32) -> i64 {
        i64::from(self.duratiion_s * bitrate)
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use regex::Regex;
    use super::*;

    #[test]
    fn check_correct_total_byte_len() {
        let duration = 60;
        let stream_url = "http://url.mp3";
        let params = FFMPEG_parameters {
            seek_time: 30,
            url: stream_url.to_string(),
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(duration, stream_url, &params);
        let bitarate = 100;
        let stream_byte_len = transcoder.total_byte_len(bitarate);

        assert_eq!(stream_byte_len, i64::from(bitarate * duration))
    }

    #[test]
    fn check_ffmpeg_command() {
        let duration = 60;
        let stream_url = "http://url.mp3";
        let params = FFMPEG_parameters {
            seek_time: 30,
            url: stream_url.to_string(),
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(duration, stream_url, &params);
        let ppath = transcoder.ffmpeg_command.get_program();
        if let Some(x) = ppath.to_str() {
            println!("{} ", x);
            assert_eq!(x, "ffmpeg");
        }
        let mut args = transcoder.ffmpeg_command.get_args();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("-ss") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-ss {}", value);
                    assert_eq!(value, params.seek_time.to_string().as_str());
                }
                Some("-i") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-i {}", value);
                    assert_eq!(value, params.url.as_str());
                }
                Some("-acodec") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-acodec {}", value);
                    assert_eq!(value, params.audio_codec.as_str());
                }
                Some("-ab") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-ab {}", value);
                }
                Some("-f") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-f {}", value);
                    assert_eq!(value, "mp3");
                }
                Some("-bufsize") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-bufsize {}", value);
                }
                Some("-maxrate") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-maxrate {}", value);
                }
                Some("pipe:stdout") => {
                    println!("pipe:stdout");
                }
                Some(x) => panic!("ffmpeg run with uknown option: {x}"),
                None => panic!("ffmpeg run with no options"),
            }
        }
        println!();
    }

    #[test]
    fn check_ffmpeg_runs() {
        let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path_to_mp3.push("src/transcoder/test.mp3");
        let duration = 60;
        let stream_url = path_to_mp3.as_os_str().to_str().unwrap();
        println!("{stream_url}");
        let params = FFMPEG_parameters {
            seek_time: 25,
            url: stream_url.to_string(),
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let mut transcoder = Transcoder::new(duration, stream_url, &params);
        match transcoder.ffmpeg_command.output() {
            Ok(x) => {
                let out = x.stderr.as_slice();
                let out = String::from_utf8_lossy(out);
                println!("command run sucessfully \nOutput: {out}");
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
        let duration = 60;
        let stream_url = path_to_mp3.as_os_str().to_str().unwrap();
        println!("{stream_url}");
        let params = FFMPEG_parameters {
            seek_time: 25,
            url: stream_url.to_string(),
            max_rate_kbit: 64,
            audio_codec: FFMPEGAudioCodec::Libmp3lame,
            bitrate_kbit: 3,
        };
        let transcoder = Transcoder::new(duration, stream_url, &params);
        let mut gen = transcoder.get_transcode_stream();
        let mut read_counts = 0;
        println!("!!printing buffer!!");
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