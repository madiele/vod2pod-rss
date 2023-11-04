use std::{collections::HashMap, net::TcpListener, time::Instant};

use actix_web::{dev::Server, guard, middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use log::{debug, error, info};
use regex::Regex;
use serde::Deserialize;
use url::Url;

use crate::{
    configs::{conf, Conf, ConfName},
    provider, rss_transcodizer,
    transcoder::{FfmpegParameters, Transcoder},
};

pub fn spawn_server(listener: TcpListener) -> eyre::Result<Server> {
    let root = conf().get(ConfName::SubfolderPath).unwrap();
    Ok(HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::new(
                middleware::TrailingSlash::MergeOnly,
            ))
            .service(
                web::scope(&root)
                    .service(
                        web::resource("transcode_media/to_mp3")
                            .name("transcode_mp3")
                            .guard(guard::Get())
                            .to(transcode_to_mp3),
                    )
                    .route("transcodize_rss", web::get().to(transcodize_rss))
                    .route("health", web::get().to(health))
                    .route("/", web::get().to(index))
                    .route("", web::get().to(index)),
            )
    })
    .listen(listener)?
    .run())
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn index(req: HttpRequest) -> HttpResponse {
    if let (Some(user_agent), Some(remote_addr), Some(referer)) = (
        req.headers().get("User-Agent"),
        req.connection_info().peer_addr(),
        req.headers().get("Referer"),
    ) {
        info!(
            "serving homepage - User-Agent: {}, Remote Address: {}, Referer: {}",
            user_agent.to_str().unwrap(),
            remote_addr.to_string(),
            referer.to_str().unwrap()
        );
    }

    let html = std::fs::read_to_string("./templates/index.html").unwrap();
    HttpResponse::Ok().content_type("text/html").body(html)
}
async fn transcodize_rss(
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>,
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

    let provider = provider::from_v2(&parsed_url);

    if !provider
        .domain_whitelist_regexes()
        .iter()
        .any(|r| r.is_match(&parsed_url.to_string()))
    {
        error!("supplied url ({parsed_url}) not in whitelist (whitelist is needed to prevent SSRF attack)");
        return HttpResponse::Forbidden().body("scheme and host not in whitelist");
    }

    let raw_rss = match provider.generate_rss_feed(parsed_url.clone()).await {
        Ok(raw_rss) => raw_rss,
        Err(e) => {
            error!("could not generate rss feed for {parsed_url}:\n{e}");
            return HttpResponse::Conflict().finish();
        }
    };
    let injected_feed = rss_transcodizer::inject_vod2pod_customizations(
        raw_rss,
        should_transcode.then(|| transcode_service_url),
    );

    let body = match injected_feed {
        Ok(body) => body,
        Err(e) => {
            error!("could not inject vod2pod customizations into generated feed");
            error!("{e}");
            return HttpResponse::Conflict().finish();
        }
    };

    let end_time = Instant::now();
    let duration = end_time - start_time;
    debug!("rss generation took {} seconds", duration.as_secs_f32());

    HttpResponse::Ok()
        .content_type("application/xml")
        .body(body)
}

#[derive(Deserialize)]
struct TranscodizeQuery {
    url: Url,
    bitrate: usize,
    duration: usize,
}

fn parse_range_header(
    content_range_str: &str,
    bytes_count: usize,
) -> eyre::Result<(usize, usize, usize)> {
    let re = Regex::new(r"(?P<start>[0-9]{1,20})-?(?P<end>[0-9]{1,20})?")?;
    let captures = if let Some(x) = re.captures_iter(content_range_str).next() {
        x
    } else {
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

    if end == start {
        return Err(eyre::eyre!(
            "The requested Rage header with a length of 0 is invalid: {content_range_str}"
        ));
    }

    let expected = (end + 1) - start;

    Ok((start, end, expected))
}

async fn transcode_to_mp3(req: HttpRequest, query: web::Query<TranscodizeQuery>) -> HttpResponse {
    let stream_url = &query.url;
    let bitrate = query.bitrate;
    let duration_secs = query.duration;
    let total_streamable_bytes = (duration_secs * bitrate * 1000) / 8;
    info!("processing transcode at {bitrate}k for {stream_url}");

    if let Ok(value) = conf().get(ConfName::TranscodingEnabled) {
        if value.eq_ignore_ascii_case("false") {
            return HttpResponse::Forbidden().finish();
        }
    }

    let provider = provider::from_v2(&stream_url);

    if !provider
        .domain_whitelist_regexes()
        .iter()
        .any(|r| r.is_match(&stream_url.to_string()))
    {
        error!("supplied url ({stream_url}) not in whitelist (whitelist is needed to prevent SSRF attack)");
        return HttpResponse::Forbidden().body("scheme and host not in whitelist");
    }

    // Range header parsing
    const DEFAULT_CONTENT_RANGE: &str = "0-";
    let content_range_str = match req.headers().get("Range") {
        Some(x) => x.to_str().unwrap_or_default(),
        None => DEFAULT_CONTENT_RANGE,
    };

    debug!("received content range {content_range_str}");

    let (start_bytes, end_bytes, expected_bytes) =
        match parse_range_header(content_range_str, total_streamable_bytes) {
            Ok((start, end, expected)) => (start, end, expected),
            Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
        };

    debug!("requested content-range: bytes {start_bytes}-{end_bytes}/{total_streamable_bytes}");

    if start_bytes > end_bytes || start_bytes > total_streamable_bytes {
        return HttpResponse::RangeNotSatisfiable().finish();
    }

    let seek_secs =
        ((start_bytes as f32) / (total_streamable_bytes as f32)) * (duration_secs as f32);
    debug!("choosen seek_time: {seek_secs}");
    let codec = conf().get(ConfName::AudioCodec).unwrap().into();
    let ffmpeg_paramenters = FfmpegParameters {
        seek_time: seek_secs,
        url: stream_url.clone(),
        audio_codec: codec,
        bitrate_kbit: bitrate,
        max_rate_kbit: bitrate * 30,
        expected_bytes_count: expected_bytes,
    };
    debug!("seconds: {duration_secs}, bitrate: {bitrate}");

    match Transcoder::new(&ffmpeg_paramenters).await {
        Ok(transcoder) => {
            let stream = transcoder.get_transcode_stream();

            let mut response_builder = if ffmpeg_paramenters.seek_time <= 0.1 {
                HttpResponse::Ok()
            } else {
                HttpResponse::PartialContent()
            };

            response_builder
                .insert_header(("Accept-Ranges", "bytes"))
                .insert_header((
                    "Content-Range",
                    format!("bytes {start_bytes}-{end_bytes}/{total_streamable_bytes}"),
                ))
                .content_type(codec.get_mime_type_str())
                .no_chunking((expected_bytes).try_into().unwrap())
                .streaming(stream)
        }
        Err(e) => HttpResponse::ServiceUnavailable().body(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_start_and_end_start_to_end() {
        let content_range_str = "bytes=0-99";
        let bytes_count = 100;
        let (start, end, expected) = parse_range_header(content_range_str, bytes_count).unwrap();
        assert_eq!((start, end, expected), (0, 99, 100));
    }

    #[test]
    fn test_get_start_and_end_middle1_to_middle2() {
        let content_range_str = "bytes=50-199";
        let bytes_count = 200;
        let (start, end, expected) = parse_range_header(content_range_str, bytes_count).unwrap();
        assert_eq!((start, end, expected), (50, 199, 150));
    }

    #[test]
    fn test_get_start_and_end_middle_to_undefined() {
        let content_range_str = "bytes=100-";
        let bytes_count = 200;
        let (start, end, expected) = parse_range_header(content_range_str, bytes_count).unwrap();
        assert_eq!((start, end, expected), (100, 199, 100));
    }

    #[test]
    fn test_get_start_and_end_start_to_undefined() {
        let content_range_str = "bytes=0-";
        let bytes_count = 200;
        let (start, end, expected) = parse_range_header(content_range_str, bytes_count).unwrap();
        assert_eq!((start, end, expected), (0, 199, 200));
    }
}

