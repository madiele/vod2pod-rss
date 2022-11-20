use actix_web::{ HttpServer, web, App, HttpResponse, guard, HttpRequest, route };
use log::{ error, info, debug };
use regex::{ Regex, Match };
use reqwest::Url;
use serde::{Serialize, Deserialize};
use simple_logger::SimpleLogger;
use uuid::Uuid;
use std::{ path::PathBuf, collections::HashMap };
use vod_to_podcast_rss::{
    transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec },
    rss_transcodizer::RssTranscodizer,
};

async fn index() -> HttpResponse {
    HttpResponse::Ok().body("server works")
}
//TODO: make into integration test
async fn test_transcode() -> HttpResponse {
    info!("starting stream");
    let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path_to_mp3.push("src/transcoder/test.mp3");
    let stream_url = path_to_mp3.as_os_str().to_str().unwrap().parse().unwrap();
    info!("{stream_url}");
    let params = FfmpegParameters {
        seek_time: 0,
        url: stream_url,
        max_rate_kbit: 64,
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: 64,
    };
    let transcoder = Transcoder::new(&params);
    let stream = transcoder.get_transcode_stream();
    HttpResponse::Ok().content_type("audio/mpeg").no_chunking(327175).streaming(stream)
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
    let rss_transcodizer = RssTranscodizer::new(url.clone(), transcode_service_url);

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
    UUID: Uuid
}

const DEFAULT_BITRATE: &str = "64";
const DEFAULT_DURATION: &str = "-1";
async fn transcode(req: HttpRequest, query: web::Query<TranscodizeQuery>) -> HttpResponse {
    let stream_url = &query.url;
    let bitrate = query.bitrate;
    let duration_secs = query.duration;
    let uuid= query.UUID;

    let bytes_count= ((duration_secs * bitrate) / 8) * 1024;
    // content-range parsing
    //Content-Range: bytes 200-1000/67589
    let default_content_range = format!("0-");
    let content_range_str = match req.headers().get("Range") {
        Some(x) => x.to_str().unwrap(),
        None => default_content_range.as_str(),
    };
    debug!("received content range {content_range_str}");

    let re = Regex::new(
        r"(?P<start>[0-9]{1,20})-?(?P<end>[0-9]{1,20})?"
    ).unwrap();
    let captures = match re.captures_iter(content_range_str).next(){
        Some(x) => x,
        None => return HttpResponse::InternalServerError().body("content range regex failed"),
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
    let seek = ( start as f32 / bytes_count as f32) * duration_secs as f32;
    debug!("choosen seek_time: {seek}");
    let ffmpeg_paramenters = FfmpegParameters {
        seek_time: seek.floor() as _,
        url: stream_url.clone(),
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: bitrate,
        max_rate_kbit: bitrate * 30,
    };
    debug!("seconds: {duration_secs}, bitrate: {bitrate}");
    

    //TODO use same uuid to kill old transcode, store pid of ffmpeg processes in redis, if new request with same uuid kill the old one
    //TODO add a timeout to the ffmpeg process if it gets stuck
    let transcoder = Transcoder::new(&ffmpeg_paramenters);
    let stream = transcoder.get_transcode_stream();

    (
        if ffmpeg_paramenters.seek_time == 0 {
            HttpResponse::Ok()
        } else {
            HttpResponse::PartialContent()
        }
    )
        .insert_header(("Accept-Ranges", "bytes"))
        .insert_header(("Content-Range", format!("bytes {start}-{end}/{bytes_count}")))
        .content_type("audio/mpeg")
        .no_chunking((bytes_count - start).into())
        .streaming(stream)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();
    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .service(
                web
                    ::resource("transcode_media/to_mp3")
                    .name("transcode_mp3")
                    .guard(guard::Get())
                    .to(transcode)
            )
            .route("/test_transcode.mp3", web::get().to(test_transcode))
            .route("/transcodize_rss", web::get().to(transcodize_rss))
    })
        .bind(("127.0.0.1", 8080))?
        .run().await
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_example() {}
}