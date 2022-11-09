use ffmpeg_next::Error;

pub struct Streams<'a> {
    duratiion_s: i32,
    stream_url: &'a str,
}

impl<'a> Streams<'a> {
    pub fn new(duratiion_s: i32, stream_url: &'a str) -> Self { Self { duratiion_s, stream_url } }

    pub fn duratiion_s(&self) -> i32 {
        self.duratiion_s
    }

    pub fn stream_url(&self) -> &str {
        self.stream_url
    }

    //TODO: to revise should take something to send the data too
    async fn transcode(&self, bitrate: i32) -> Result<i32, Error> {
        todo!()
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
        let stream = Streams::new(duration, stream_url);
        let bitarate = 100;
        let stream_byte_len = stream.total_byte_len(bitarate);

        assert_eq!(stream_byte_len, i64::from(bitarate * duration))
    }
}