use std::process::Command;

use actix_web::{dev::ServiceRequest, web::Bytes};
use anyhow::Error;
use futures::Future;
use genawaiter::{yield_, sync::{Gen}, sync_producer};

enum FFMPEG_audio_codec {
    Libmp3lame
}

impl<'a> FFMPEG_audio_codec {
    fn as_str(self) -> &'a str {
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

pub struct Streaming_settings {
    
}

pub struct Transcoder<'a> {
    duratiion_s: i32,
    stream_url: &'a str,
    ffmpeg_command: Command
}

impl<'a> Transcoder<'a> {
    pub fn new(duratiion_s: i32, stream_url: &'a str, ffmpeg_paramenters: FFMPEG_parameters) -> Self {
        let mut command = Command::new("sh");
        Self::set_ffmpeg_command(&mut command, ffmpeg_paramenters);
        Self {
            duratiion_s,
            stream_url,
            ffmpeg_command: command,
        }
    }
    
    fn set_ffmpeg_command(command: &mut Command, ffmpeg_paramenters: FFMPEG_parameters) -> &mut Command {
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
        let stream = Transcoder::new(duration, stream_url,
        FFMPEG_parameters { seek_time: 2, url: "laksjd".to_string(), max_rate_kbit: 2, buffer_size: 2, transcode_bandwith: 2, audio_codec: FFMPEG_audio_codec::Libmp3lame, bitrate_kbit: 3 });
        let bitarate = 100;
        let stream_byte_len = stream.total_byte_len(bitarate);

        assert_eq!(stream_byte_len, i64::from(bitarate * duration))
    }
}