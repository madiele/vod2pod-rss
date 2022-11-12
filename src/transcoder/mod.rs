use std::process::Command;

use actix_web::{dev::ServiceRequest, web::Bytes};
use anyhow::Error;
use futures::Future;
use genawaiter::{yield_, sync::{Gen}, sync_producer};

enum FFMPEG_audio_codec {
    Libmp3lame
}

impl FFMPEG_audio_codec {
    fn as_str(&self) -> &'static str {
        match self {
            FFMPEG_audio_codec::Libmp3lame => "libmp3lame",
        }
    }
}

impl Default for FFMPEG_audio_codec {
    fn default() -> Self {
        Self::Libmp3lame
    }
}

pub struct FFMPEG_parameters {
    seek_time: u32,
    url: String,
    audio_codec: FFMPEG_audio_codec,
    bitrate_kbit: u32,
    max_rate_kbit: u32,
    buffer_size: u32,
    transcode_bandwith: u32,
}

impl FFMPEG_parameters {
    pub fn bitarate(&self) -> u64 {u64::from(self.bitrate_kbit * 1024)}
}

pub struct Streaming_settings {
    
}

pub struct Transcoder<'a> {
    duratiion_s: i32,
    stream_url: &'a str,
    ffmpeg_command: Command
}

impl<'a> Transcoder<'a> {
    pub fn new(duratiion_s: i32, stream_url: &'a str, ffmpeg_paramenters: &FFMPEG_parameters) -> Self {
        let mut command = Command::new("ffmpeg");
        Self::set_ffmpeg_command(&mut command, ffmpeg_paramenters);
        Self {
            duratiion_s,
            stream_url,
            ffmpeg_command: command,
        }
    }
    
    fn set_ffmpeg_command<'b>(command: &'b mut Command, ffmpeg_paramenters: &'b FFMPEG_parameters) -> &'b mut Command {
        command.args(["-ss", ffmpeg_paramenters.seek_time.to_string().as_str()])
            .args(["-i", ffmpeg_paramenters.url.as_str()])
            .args(["-acodec", ffmpeg_paramenters.audio_codec.as_str()])
            .args(["-ab", format!("{}k", (ffmpeg_paramenters.bitrate_kbit/1000)).as_str()])
            .args(["-f","mp3"])
            .args(["-bufsize", (ffmpeg_paramenters.bitrate_kbit * 30).to_string().as_str()])
            .args(["-maxrate", format!("{}k", ffmpeg_paramenters.transcode_bandwith).as_str()])
            .args(["pipe:stdout"])
    }

    pub fn duratiion_s(&self) -> i32 {
        self.duratiion_s
    }

    pub fn stream_url(&self) -> &str {
        self.stream_url
    }

    //TODO: to revise should take something to send the data too
    async fn start_streaming_to_client(&self, request: ServiceRequest, streaming_settings: &Streaming_settings) -> Result<i32, Error> {
        todo!()
    }
    
    fn get_transcode_generator(&self, streaming_settings: &Streaming_settings) -> Gen<Bytes, (), impl Future>  {
        let odd_numbers_less_than_ten = sync_producer!({
            let n = "alksjdlkasjdlkjsadlkjdsa";
            while true {
                yield_!(Bytes::from(n)); // Suspend a function at any point with a value.
            }
        });
        Gen::new(odd_numbers_less_than_ten)
    }

    fn total_byte_len(&self, bitrate: i32) -> i64 { i64::from(self.duratiion_s * bitrate) }
}

#[cfg(test)]
mod test {
    use super::*;

    # [test]
    fn check_correct_total_byte_len() {
        let duration = 60;
        let stream_url = "http://url.mp3";
        let params = FFMPEG_parameters { seek_time: 30, url: stream_url.to_string(), max_rate_kbit: 64, buffer_size: 1000, transcode_bandwith: 2, audio_codec: FFMPEG_audio_codec::Libmp3lame, bitrate_kbit: 3 };
        let transcoder = Transcoder::new(duration, stream_url, &params);
        let bitarate = 100;
        let stream_byte_len = transcoder.total_byte_len(bitarate);

        assert_eq!(stream_byte_len, i64::from(bitarate * duration))
    }
    
    #[test]
    fn check_ffmpeg_command() {
        let duration = 60;
        let stream_url = "http://url.mp3";
        let params = FFMPEG_parameters { seek_time: 30, url: stream_url.to_string(), max_rate_kbit: 64, buffer_size: 1000, transcode_bandwith: 2, audio_codec: FFMPEG_audio_codec::Libmp3lame, bitrate_kbit: 3 };
        let transcoder = Transcoder::new(duration, stream_url, &params);
        let ppath = transcoder.ffmpeg_command.get_program();
        if let Some(x) = ppath.to_str() {
            println!("{} ",x);
            assert_eq!(x, "ffmpeg");
        }        
        let mut args = transcoder.ffmpeg_command.get_args();

        while let Some(arg) = args.next()  {
            match arg.to_str() {
                Some("-ss") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-ss {}", value);
                    assert_eq!(value, params.seek_time.to_string().as_str());
                },
                Some("-i") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-i {}", value);
                    assert_eq!(value, params.url.as_str());
                },
                Some("-acodec") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-acodec {}", value);
                    assert_eq!(value, params.audio_codec.as_str());
                },
                Some("-ab") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-ab {}", value);
                },
                Some("-f") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-f {}", value);
                    assert_eq!(value, "mp3");
                },
                Some("-bufsize") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-bufsize {}", value);
                },
                Some("-maxrate") => {
                    let value = args.next().unwrap().to_str().unwrap();
                    println!("-maxrate {}", value);
                },
                Some("pipe:stdout") => { println!("pipe:stdout") },
                Some(x) => panic!("ffmpeg run with uknown option: {x}"),
                None => panic!("ffmpeg run with no options"),
            }
        }
        println!();
    }
}