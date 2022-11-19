use actix_web::{ HttpServer, web, App, HttpResponse, guard, HttpRequest };
use log::{error, info};
use reqwest::Url;
use simple_logger::SimpleLogger;
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

const DEFAULT_BITRATE: &str = "64";
const DEFAULT_DURATION: &str = "-1";
async fn transcode(query: web::Query<HashMap<String, String>>) -> HttpResponse {
    let empty_string = "".to_string();
    let stream_url = match ({ Url::parse(query.get("url").unwrap_or(&empty_string)) }) {
        Ok(x) => x,
        Err(e) => {
            error!("can't parse URL");
            return HttpResponse::BadRequest().body("can't parse URL");
        }
    };

    let bitrate = match query.get("bitrate").unwrap_or(&DEFAULT_BITRATE.to_string()).parse() {
        Ok(x) => x,
        Err(_) => {
            error!("can't parse bitrate");
            return HttpResponse::BadRequest().body("can't parse bitrate");
        }
    };

    let duration_secs: u32 = match
        query.get("duration").unwrap_or(&DEFAULT_DURATION.to_string()).parse()
    {
        Ok(x) => x,
        Err(_) => {
            error!("can't parse duration");
            return HttpResponse::BadRequest().body("can't parse duration");
        }
    };

    let ffmpeg_paramenters = FfmpegParameters {
        seek_time: 0,
        url: stream_url,
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: bitrate,
        max_rate_kbit: bitrate * 30,
    };
    let transcoder = Transcoder::new(&ffmpeg_paramenters);
    let stream = transcoder.get_transcode_stream();
    HttpResponse::Ok()
        .content_type("audio/mpeg")
        .no_chunking((duration_secs * bitrate).into())
        .streaming(stream)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    SimpleLogger::new().with_level(log::LevelFilter::Info).init().unwrap();
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