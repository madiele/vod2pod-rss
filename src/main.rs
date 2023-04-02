use actix::Addr;
use actix_redis::{ RedisActor, Command, resp_array, RespValue };
use actix_web::{ HttpServer, web::{ self, Buf }, App, HttpResponse, guard, HttpRequest };
use async_trait::async_trait;
use log::{ error, debug };
use regex::Regex;
use reqwest::Url;
use serde::Deserialize;
use serde_resp::de;
use simple_logger::SimpleLogger;
use uuid::Uuid;
use std::{io::Cursor, process::Child};
use std::collections::HashMap;
use vod_to_podcast_rss::{
    transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec },
    rss_transcodizer::RssTranscodizer,
};


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();

    let redis_url = match std::env::var_os("REDIS_URL") {
        Some(x) => x.into_string().unwrap(),
        None => "localhost:6379".to_string(),
    };
    let redis = RedisActor::start(redis_url.as_str());
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(redis.clone()))
            .route("/", web::get().to(index))
            .service(
                web
                    ::resource("transcode_media/to_mp3")
                    .name("transcode_mp3")
                    .guard(guard::Get())
                    .to(transcode)
            )
            //.route("/test_transcode.mp3", web::get().to(test_transcode))
            .route("/transcodize_rss", web::get().to(transcodize_rss))
            .route("transcode_media/to_mp3", web::head().to(|| async move {
                debug!("HEAD request");
                HttpResponse::Ok()
            }))
    })
        .bind(("0.0.0.0", 8080))?
        .run().await
}

#[async_trait]
trait RedisActorExtensions {
    /// example redis.sendAndRecString(Command(resp_array!["INFO"])).await
    ///
    /// # Panics
    ///
    /// Panics if .
    async fn send_and_rec_string(&self, msg: actix_redis::Command) -> Option<String>;
}

#[async_trait]
impl RedisActorExtensions for web::Data<Addr<RedisActor>> {
    async fn send_and_rec_string(&self, msg: actix_redis::Command) -> Option<String>
    {
        if let Ok(Ok(RespValue::BulkString(x))) = self.send(msg).await {
            Some(x.into_string())
        } else {
            None
        }
    }
}

async fn index(redis: web::Data<Addr<RedisActor>>) -> HttpResponse {
    HttpResponse::Ok().body("server works")
}

//TODO: make into integration test
// async fn test_transcode() -> HttpResponse {
//     info!("starting stream");
//     let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//     path_to_mp3.push("src/transcoder/test.mp3");
//     let stream_url = path_to_mp3.as_os_str().to_str().unwrap().parse().unwrap();
//     info!("{stream_url}");
//     let params = FfmpegParameters {
//         seek_time: 0,
//         url: stream_url,
//         max_rate_kbit: 64,
//         audio_codec: FFMPEGAudioCodec::Libmp3lame,
//         bitrate_kbit: 64,
//     };
//     let transcoder = Transcoder::new(&params);
//     let stream = transcoder.get_transcode_stream();
//     HttpResponse::Ok().content_type("audio/mpeg").no_chunking(327175).streaming(stream)
// }

async fn transcodize_rss(
    redis: web::Data<Addr<RedisActor>>,
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>
) -> HttpResponse {
    let url = if let Some(x) = query.get("url") {
        x
    } else {
        error!("no url provided");
        return HttpResponse::BadRequest().finish();
    };

    let transcode_service_url = req.url_for("transcode_mp3", &[""]).unwrap();

    //TODO cache with lifetime 1 minute
    let rss_transcodizer = RssTranscodizer::new(url.to_string(), transcode_service_url);

    let body = match rss_transcodizer.transcodize().await {
        Ok(body) => body,
        Err(e) => {
            error!("could not translate rss");
            error!("{e}");
            return HttpResponse::Conflict().finish();
        }
    };

    HttpResponse::Ok().content_type("application/xml").body(body)
}


#[derive(Deserialize)]
struct TranscodizeQuery {
    url: Url,
    bitrate: u32,
    duration: u32,
    uuid: Uuid,
}

async fn transcode(
    req: HttpRequest,
    query: web::Query<TranscodizeQuery>,
    redis: web::Data<Addr<RedisActor>>
) -> HttpResponse {
    let stream_url = &query.url;
    let bitrate = query.bitrate;
    let duration_secs = query.duration;
    let bytes_count = ((duration_secs * bitrate) / 8) * 1024;

    if let Ok(Ok(res)) = redis.send(Command(resp_array!["INFO"])).await {
        let b = match res {
            RespValue::Nil => todo!(),
            RespValue::Array(_) => todo!(),
            RespValue::BulkString(x) => x,
            RespValue::Error(_) => todo!(),
            RespValue::Integer(_) => todo!(),
            RespValue::SimpleString(_) => todo!(),
        };
        let buff = &mut Cursor::new(b);
        let pars: Result<String, _> = de::from_buf_reader(buff);
        debug!("redis res: {}", pars.unwrap());
    } else {
        debug!("redis is down");
    }

    // Range header parsing
    const DEFAULT_CONTENT_RANGE: &str = "0-";
    let content_range_str = match req.headers().get("Range") {
        Some(x) => x.to_str().unwrap(),
        None => DEFAULT_CONTENT_RANGE,
    };
    debug!("received content range {content_range_str}");

    let re = Regex::new(r"(?P<start>[0-9]{1,20})-?(?P<end>[0-9]{1,20})?").unwrap();
    let captures = match re.captures_iter(content_range_str).next() {
        Some(x) => x,
        None => {
            return HttpResponse::InternalServerError().body("content range regex failed");
        }
    };

    let mut start = 0;
    if let Some(x) = captures.name("start") {
        start = x.as_str().parse().unwrap();
    }
    let mut end = bytes_count - 1;
    if let Some(x) = captures.name("end") {
        end = x.as_str().parse().unwrap();
    }

    debug!("requested content-range: bytes {start}-{end}/{bytes_count}");

    if start > end || start > bytes_count {
        return HttpResponse::RangeNotSatisfiable().finish();
    }
    let seek = ((start as f32) / (bytes_count as f32)) * (duration_secs as f32);
    debug!("choosen seek_time: {seek}");
    let ffmpeg_paramenters = FfmpegParameters {
        seek_time: seek.floor() as _,
        url: stream_url.clone(),
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: bitrate,
        max_rate_kbit: bitrate * 30,
    };
    debug!("seconds: {duration_secs}, bitrate: {bitrate}");

    let transcoder = Transcoder::new(&ffmpeg_paramenters);
    let stream = transcoder.get_transcode_stream(redis);

    (
        if ffmpeg_paramenters.seek_time == 0 {
            HttpResponse::Ok()
        } else {
            HttpResponse::PartialContent()
        }
    ).insert_header(("Accept-Ranges", "bytes"))
        .insert_header(("Content-Range", format!("bytes {start}-{end}/{bytes_count}")))
        .content_type("audio/mpeg")
        .no_chunking((bytes_count - start).into())
        .streaming(stream)
}

trait IntoString {
    fn into_string(&self) -> String;
}

impl IntoString for Vec<u8> {
    fn into_string(&self) -> String {
        let mut result = String::with_capacity(self.len());
        let mut cursor = Cursor::new(self);
        while cursor.has_remaining() {
            if cursor.position() != 0 {
                result.push_str("\n");
            }
            let line: Result<String, _> = de::from_buf_reader(&mut cursor);
            result.push_str(line.unwrap().as_str());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_example() {}
}
