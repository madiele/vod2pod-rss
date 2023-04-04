use actix::Addr;
use actix_redis::{ RedisActor, RespValue };
use actix_web::{ HttpServer, web::{ self, Buf }, App, HttpResponse, guard, HttpRequest };
use async_trait::async_trait;
use log::{ error, debug, info };
use regex::Regex;
use reqwest::Url;
use serde::Deserialize;
use serde_resp::de;
use simple_logger::SimpleLogger;
use std::{io::Cursor, collections::HashMap};
use vod_to_podcast_rss::{
    transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec }, rss_transcodizer::RssTranscodizer,
};


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();

    let redis_address = std::env::var("REDIS_ADDRESS").unwrap_or_else(|_| "localhost".to_string());
    let redis_port = std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());
    let redis_actor = RedisActor::start(format!("{redis_address}:{redis_port}"));

    flush_redis_on_new_version().await.unwrap();
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(redis_actor.clone()))
            .route("/", web::get().to(index))
            .service(
                web
                    ::resource("transcode_media/to_mp3")
                    .name("transcode_mp3")
                    .guard(guard::Get())
                    .to(transcode)
            )
            .route("/transcodize_rss", web::get().to(transcodize_rss))
    })
        .bind(("0.0.0.0", 8080))?
        .run().await
}

async fn flush_redis_on_new_version() -> eyre::Result<()> {
    let redis_address = std::env::var("REDIS_ADDRESS").unwrap_or_else(|_| "localhost".to_string());
    let redis_port = std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());
    let app_version = env!("CARGO_PKG_VERSION");
    info!("app version {app_version}");

    let client = redis::Client::open(format!("redis://{}:{}/", redis_address, redis_port))?;
    let mut con = client.get_tokio_connection().await.unwrap();

    let cached_version : Option<String> = redis::cmd("GET").arg("version").query_async(&mut con).await?;
    debug!("cached app version {:?}", cached_version);

    if let Some(ref cached_version) = cached_version {
        if cached_version != app_version {
            info!("detected version change ({cached_version} != {app_version}) flushing redis DB");
            let _ : () = redis::cmd("FLUSHDB").query_async(&mut con).await?;
        }
    }


    let _ : () = redis::cmd("SET").arg("version").arg(app_version).query_async(&mut con).await?;
    debug!("set cached app version to {app_version}");

    Ok(())
}

#[async_trait]
trait RedisActorExtensions {
    /// example redis.sendAndRecString(Command(resp_array!["INFO"])).await
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

async fn index() -> HttpResponse {
    HttpResponse::Ok().body("server works")
}

async fn transcodize_rss(
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
    info!("processing transcode at {bitrate}k for {stream_url}");

    // Range header parsing
    const DEFAULT_CONTENT_RANGE: &str = "0-";
    let content_range_str = match req.headers().get("Range") {
        Some(x) => x.to_str().unwrap(),
        None => DEFAULT_CONTENT_RANGE,
    };

    debug!("received content range {content_range_str}");

    let re = Regex::new(r"(?P<start>[0-9]{1,20})-?(?P<end>[0-9]{1,20})?").unwrap();
    let captures = if let Some(x) = re.captures_iter(content_range_str).next() {x} else {
        return HttpResponse::BadRequest().body("content range regex failed");
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

    match Transcoder::new(&ffmpeg_paramenters).await {
        Ok(transcoder) => {
            let stream = transcoder.get_transcode_stream(redis);

            let mut response_builder = match ffmpeg_paramenters.seek_time {
                0 => HttpResponse::Ok(),
                _ => HttpResponse::PartialContent(),
            };
            response_builder.insert_header(("Accept-Ranges", "bytes"))
                .insert_header(("Content-Range", format!("bytes {start}-{end}/{bytes_count}")))
                .content_type("audio/mpeg")
                .no_chunking((bytes_count - start).into())
                .streaming(stream)
        }
        Err(e) => {
            HttpResponse::ServiceUnavailable().body(e.to_string())
        },
    }
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
}
