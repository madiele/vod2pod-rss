use std::{collections::HashMap, net::TcpListener, time::Instant};

use actix_web::{
    body::SizedStream, dev::Server, guard, http, middleware, web, App, HttpRequest, HttpResponse,
    HttpResponseBuilder, HttpServer,
};
use futures::stream;
use log::{debug, error, info, warn};
use serde::Deserialize;
use url::Url;

use crate::{
    configs::{conf, Conf, ConfName},
    provider::{self, MediaProvider},
    rss_transcodizer,
    transcoder::{estimated_output_bytes, FfmpegParameters, Transcoder},
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
                        web::resource("transcode_media/to.mp3")
                            .name("transcode_mp3")
                            .guard(guard::Any(guard::Get()).or(guard::Head()))
                            .to(transcode_to_mp3),
                    )
                    .service(
                        //this is an old URL used in old vod2pod versions that did not work with
                        //itunes kept for backwards compatiility
                        web::resource("transcode_media/to_mp3")
                            .name("transcode_mp3_obsolete")
                            .guard(guard::Any(guard::Get()).or(guard::Head()))
                            .to(transcode_to_mp3),
                    )
                    .route("transcodize_rss", web::get().to(transcodize_rss))
                    .route("transcodize_rss", web::head().to(transcodize_rss))
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
    if req.method() == http::Method::HEAD {
        return HttpResponse::Ok().finish();
    }

    let start_time = Instant::now();

    let should_transcode = match conf().get(ConfName::TranscodingEnabled) {
        Ok(value) => !value.eq_ignore_ascii_case("false"),
        Err(_) => true,
    };

    if !should_transcode {
        warn!("transcoding is disabled");
    }
    let url = if let Some(x) = query.get("url") {
        x
    } else {
        error!("no url provided");
        return HttpResponse::BadRequest().finish();
    };

    let transcode_service_url = req.url_for("transcode_mp3", [""]).unwrap();

    let parsed_url = match Url::parse(url) {
        Ok(x) => x,
        Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
    };

    let provider = provider::from(&parsed_url);

    if !provider
        .domain_whitelist_regexes()
        .iter()
        .any(|r| r.is_match(parsed_url.as_ref()))
    {
        error!("supplied url ({parsed_url}) not in whitelist (whitelist is needed to prevent SSRF attack)");
        return HttpResponse::Forbidden().body("scheme and host not in whitelist");
    }

    //check cache
    let Ok(mut redis) = crate::get_redis_client().await else {
        error!("could not get redis client");
        return HttpResponse::InternalServerError().finish();
    };

    let cached_rss: Option<String> = redis::cmd("GET")
        .arg(&parsed_url.to_string())
        .query_async(&mut redis)
        .await
        .unwrap_or_default();

    if let Some(cached_rss) = cached_rss {
        info!("serving cached rss feed for {parsed_url}");
        return HttpResponse::Ok()
            .content_type("application/xml")
            .body(cached_rss);
    }

    //generate rss feed
    let raw_rss = match provider.generate_rss_feed(parsed_url.clone()).await {
        Ok(raw_rss) => raw_rss,
        Err(e) => {
            error!("could not generate rss feed for {parsed_url}:\n{e}");
            return HttpResponse::Conflict().finish();
        }
    };

    // rewrite urls in feed
    let injected_feed = rss_transcodizer::inject_vod2pod_customizations(
        raw_rss,
        should_transcode.then_some(transcode_service_url),
    );

    let body = match injected_feed {
        Ok(body) => body,
        Err(e) => {
            error!("could not inject vod2pod customizations into generated feed");
            error!("{e}");
            return HttpResponse::Conflict().finish();
        }
    };

    //set cache to env var CACHE_TTL (or default 600 seconds)
    let cache_ttl: u64 = match conf().get(ConfName::CacheTTL) {
        Ok(value) => value.parse().unwrap_or(600),
        Err(_) => 600,
    };
    let _: () = redis::cmd("SET")
        .arg(&parsed_url.to_string())
        .arg(&body)
        .arg("EX")
        .arg(cache_ttl)
        .query_async(&mut redis)
        .await
        .unwrap_or_default();

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

#[derive(Debug, PartialEq)]
enum RangeError {
    Invalid,
    Unsatisfiable,
}

fn parse_range_header(range_header: &str, bytes_count: u64) -> Result<(u64, u64, u64), RangeError> {
    if bytes_count == 0 {
        return Err(RangeError::Unsatisfiable);
    }

    let range = range_header
        .trim()
        .strip_prefix("bytes=")
        .ok_or(RangeError::Invalid)?;
    if range.contains(',') {
        return Err(RangeError::Invalid);
    }

    let (start, end) = range.split_once('-').ok_or(RangeError::Invalid)?;
    let (start, end) = match (start, end) {
        ("", "") => return Err(RangeError::Invalid),
        ("", suffix) => {
            let suffix = suffix.parse::<u64>().map_err(|_| RangeError::Invalid)?;
            if suffix == 0 {
                return Err(RangeError::Invalid);
            }
            (bytes_count.saturating_sub(suffix), bytes_count - 1)
        }
        (start, "") => {
            let start = start.parse::<u64>().map_err(|_| RangeError::Invalid)?;
            if start >= bytes_count {
                return Err(RangeError::Unsatisfiable);
            }
            (start, bytes_count - 1)
        }
        (start, end) => {
            let start = start.parse::<u64>().map_err(|_| RangeError::Invalid)?;
            let end = end.parse::<u64>().map_err(|_| RangeError::Invalid)?;
            if start >= bytes_count || end < start {
                return Err(RangeError::Unsatisfiable);
            }
            (start, end.min(bytes_count - 1))
        }
    };

    Ok((start, end, end - start + 1))
}

fn media_response_builder(
    is_partial: bool,
    start_bytes: u64,
    end_bytes: u64,
    total_bytes: u64,
    content_length: u64,
    content_type: &str,
) -> HttpResponseBuilder {
    let mut response = if is_partial {
        HttpResponse::PartialContent()
    } else {
        HttpResponse::Ok()
    };

    response
        .insert_header((http::header::ACCEPT_RANGES, "bytes"))
        .no_chunking(content_length)
        .content_type(content_type);

    if is_partial {
        response.insert_header((
            http::header::CONTENT_RANGE,
            format!("bytes {start_bytes}-{end_bytes}/{total_bytes}"),
        ));
    }

    response
}

fn head_media_response(mut response: HttpResponseBuilder, content_length: u64) -> HttpResponse {
    let empty_body = stream::empty::<Result<web::Bytes, actix_web::Error>>();
    response.body(SizedStream::new(content_length, empty_body))
}

async fn transcode_to_mp3(req: HttpRequest, query: web::Query<TranscodizeQuery>) -> HttpResponse {
    let stream_url = &query.url;
    let bitrate = query.bitrate;
    let duration_secs = query.duration;
    let total_streamable_bytes = estimated_output_bytes(duration_secs as u64, bitrate as u64);
    info!("processing transcode at {bitrate}k for {stream_url}");

    if let Ok(value) = conf().get(ConfName::TranscodingEnabled) {
        if value.eq_ignore_ascii_case("false") {
            return HttpResponse::Forbidden().finish();
        }
    }

    let provider = provider::from(stream_url);

    if !provider
        .domain_whitelist_regexes()
        .iter()
        .any(|r| r.is_match(stream_url.as_ref()))
    {
        error!("supplied url ({stream_url}) not in whitelist (whitelist is needed to prevent SSRF attack)");
        return HttpResponse::Forbidden().body("scheme and host not in whitelist");
    }

    let range_header = req.headers().get(http::header::RANGE);
    let is_partial = range_header.is_some();
    let (start_bytes, end_bytes, expected_bytes) = match range_header {
        Some(value) => match value
            .to_str()
            .map_err(|_| RangeError::Invalid)
            .and_then(|value| parse_range_header(value, total_streamable_bytes))
        {
            Ok(range) => range,
            Err(RangeError::Invalid) => return HttpResponse::BadRequest().finish(),
            Err(RangeError::Unsatisfiable) => {
                return HttpResponse::RangeNotSatisfiable()
                    .insert_header((
                        http::header::CONTENT_RANGE,
                        format!("bytes */{total_streamable_bytes}"),
                    ))
                    .finish()
            }
        },
        None if total_streamable_bytes > 0 => {
            (0, total_streamable_bytes - 1, total_streamable_bytes)
        }
        None => return HttpResponse::NoContent().finish(),
    };

    debug!("requested content-range: bytes {start_bytes}-{end_bytes}/{total_streamable_bytes}");

    let seek_secs =
        ((start_bytes as f64) / (total_streamable_bytes as f64)) * (duration_secs as f64);
    debug!("choosen seek_time: {seek_secs}");

    let timeout_in_seconds = conf()
        .get(ConfName::FfmpegTimeoutSeconds)
        .unwrap()
        .parse()
        .unwrap();
    debug!("choosen timeout in seconds: {timeout_in_seconds}");

    let codec = conf().get(ConfName::AudioCodec).unwrap().into();
    let ffmpeg_paramenters = FfmpegParameters {
        seek_time: seek_secs as f32,
        url: stream_url.clone(),
        audio_codec: codec,
        bitrate_kbit: bitrate,
        max_rate_kbit: bitrate * 30,
        expected_bytes_count: match expected_bytes.try_into() {
            Ok(expected_bytes) => expected_bytes,
            Err(_) => return HttpResponse::InternalServerError().finish(),
        },
        timeout_in_seconds: timeout_in_seconds,
    };
    debug!("seconds: {duration_secs}, bitrate: {bitrate}");

    if req.method() == http::Method::HEAD {
        return head_media_response(
            media_response_builder(
                is_partial,
                start_bytes,
                end_bytes,
                total_streamable_bytes,
                expected_bytes,
                codec.get_mime_type_str(),
            ),
            expected_bytes,
        );
    }

    match Transcoder::new(&ffmpeg_paramenters).await {
        Ok(transcoder) => {
            let stream = transcoder.get_transcode_stream();

            media_response_builder(
                is_partial,
                start_bytes,
                end_bytes,
                total_streamable_bytes,
                expected_bytes,
                codec.get_mime_type_str(),
            )
            .streaming(stream)
        }
        Err(e) => HttpResponse::ServiceUnavailable().body(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::body::{BodySize, MessageBody};

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

    #[test]
    fn test_get_single_byte_range() {
        assert_eq!(parse_range_header("bytes=0-0", 100), Ok((0, 0, 1)));
    }

    #[test]
    fn test_get_suffix_range() {
        assert_eq!(parse_range_header("bytes=-25", 100), Ok((75, 99, 25)));
    }

    #[test]
    fn test_range_end_is_clamped_to_content_length() {
        assert_eq!(parse_range_header("bytes=75-200", 100), Ok((75, 99, 25)));
    }

    #[test]
    fn test_range_start_beyond_content_is_unsatisfiable() {
        assert_eq!(
            parse_range_header("bytes=100-", 100),
            Err(RangeError::Unsatisfiable)
        );
    }

    #[test]
    fn full_response_has_content_length_without_content_range() {
        let response = media_response_builder(false, 0, 99, 100, 100, "audio/mpeg").finish();

        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .unwrap(),
            "100"
        );
        assert!(!response.headers().contains_key(http::header::CONTENT_RANGE));
    }

    #[test]
    fn partial_response_has_range_status_and_headers() {
        let response = media_response_builder(true, 0, 0, 100, 1, "audio/mpeg").finish();

        assert_eq!(response.status(), http::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .unwrap(),
            "1"
        );
        assert_eq!(
            response.headers().get(http::header::CONTENT_RANGE).unwrap(),
            "bytes 0-0/100"
        );
    }

    #[test]
    fn head_response_preserves_the_declared_body_size() {
        let response = head_media_response(
            media_response_builder(false, 0, 99, 100, 100, "audio/mpeg"),
            100,
        );

        assert_eq!(response.body().size(), BodySize::Sized(100));
    }
}
