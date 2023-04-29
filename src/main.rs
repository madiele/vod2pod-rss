use actix_web::{ middleware, HttpServer, web, App, HttpResponse, guard, HttpRequest };
use log::{ error, debug, info };
use regex::Regex;
use reqwest::Url;
use serde::Deserialize;
use simple_logger::SimpleLogger;
use std::{collections::HashMap, time::Instant};
use vod2pod_rss::{
    transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec }, rss_transcodizer::RssTranscodizer, url_convert, configs::{Conf, conf, ConfName},
};

//cache test

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    SimpleLogger::new().with_level(log::LevelFilter::Info).env().init().unwrap();
    let root = conf().get(ConfName::SubfolderPath).unwrap();

    if let Err(err) = flush_redis_on_new_version().await {
        panic!("Error interacting with Redis (redis is required): {:?}", err);
    }

    info!("starting server at http://0.0.0.0:8080{}", root);
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(middleware::TrailingSlash::MergeOnly))
            .service(web::scope(&root)
                .service(web::resource("transcode_media/to_mp3")
                    .name("transcode_mp3")
                    .guard(guard::Get())
                    .to(transcode))
                .route("transcodize_rss", web::get().to(transcodize_rss))
                .route("/", web::get().to(index))
                .route("", web::get().to(index))
            )
    })
        .bind(("0.0.0.0", 8080))?
        .run().await
}

async fn flush_redis_on_new_version() -> eyre::Result<()> {
    let redis_address = conf().get(ConfName::RedisAddress).unwrap();
    let redis_port = conf().get(ConfName::RedisPort).unwrap();
    let app_version = env!("CARGO_PKG_VERSION");
    info!("app version {app_version}");

    let client = redis::Client::open(format!("redis://{}:{}/", redis_address, redis_port))?;
    let mut con = client.get_tokio_connection().await?;

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

async fn index(req: HttpRequest) -> HttpResponse {
    if let (Some(user_agent), Some(remote_addr), Some(referer)) = (req.headers().get("User-Agent"), req.connection_info().peer_addr(), req.headers().get("Referer")) {
        info!("serving homepage - User-Agent: {}, Remote Address: {}, Referer: {}", user_agent.to_str().unwrap(), remote_addr.to_string(), referer.to_str().unwrap());
    }

    let html = std::fs::read_to_string("./templates/index.html").unwrap();
    HttpResponse::Ok().content_type("text/html").body(html)
}


async fn transcodize_rss(
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>
) -> HttpResponse {
    let start_time = Instant::now();

    let should_transcode = match conf().get(ConfName::TranscodingEnabled) {
        Ok(value) => !value.eq_ignore_ascii_case("false"),
        Err(_) => true,
    };

    let url = if let Some(x) = query.get("url") {
        x
    } else {
        error!("no url provided");
        return HttpResponse::BadRequest().finish();
    };

    let transcode_service_url = req.url_for("transcode_mp3", &[""]).unwrap();

    let parsed_url = match Url::parse(url) {
        Ok(x) => x,
        Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
    };

    let converted_url = match url_convert::from(parsed_url).to_feed_url().await {
        Ok(x) => x,
        Err(e) => {error!("fail when trying to convert channel {e}"); return HttpResponse::BadRequest().body(e.to_string())},
    };

    let rss_transcodizer = RssTranscodizer::new(converted_url, transcode_service_url, should_transcode).await;

    let body = match rss_transcodizer.transcodize().await {
        Ok(body) => body,
        Err(e) => {
            error!("could not translate rss");
            error!("{e}");
            return HttpResponse::Conflict().finish();
        }
    };

    let end_time = Instant::now();
    let duration = end_time - start_time;
    debug!("rss generation took {} seconds", duration.as_secs_f32());

    HttpResponse::Ok().content_type("application/xml").body(body)
}


#[derive(Deserialize)]
struct TranscodizeQuery {
    url: Url,
    bitrate: u32,
    duration: u32,
}

fn get_start_and_end(content_range_str: &str, bytes_count: u32) -> eyre::Result<(u32, u32)> {
    let re = Regex::new(r"(?P<start>[0-9]{1,20})-?(?P<end>[0-9]{1,20})?")?;
    let captures = if let Some(x) = re.captures_iter(content_range_str).next() { x } else {
        return Err(eyre::eyre!("content range regex failed"));
    };

    let mut start = 0;
    if let Some(x) = captures.name("start") {
        start = x.as_str().parse()?;
    }
    let mut end = bytes_count - 1;
    if let Some(x) = captures.name("end") {
        end = x.as_str().parse()?;
    }

    Ok((start, end))
}

async fn transcode(
    req: HttpRequest,
    query: web::Query<TranscodizeQuery>,
) -> HttpResponse {
    let stream_url = &query.url;
    let bitrate = query.bitrate;
    let duration_secs = query.duration;
    let bytes_count = ((duration_secs * bitrate) / 8) * 1024;
    info!("processing transcode at {bitrate}k for {stream_url}");

    if let Ok(value) = conf().get(ConfName::TranscodingEnabled) {
        if value.eq_ignore_ascii_case("false") {
            return HttpResponse::Forbidden().finish();
        }
    }

    // Range header parsing
    const DEFAULT_CONTENT_RANGE: &str = "0-";
    let content_range_str = match req.headers().get("Range") {
        Some(x) => x.to_str().unwrap_or_default(),
        None => DEFAULT_CONTENT_RANGE,
    };

    debug!("received content range {content_range_str}");


    let (start, end) = match get_start_and_end(content_range_str, bytes_count) {
        Ok((start, end)) => (start, end),
        Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
    };

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
            let stream = transcoder.get_transcode_stream();

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

#[cfg(test)]
mod tests {
}
