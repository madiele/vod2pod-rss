use actix_web::{ HttpServer, web, App, HttpResponse, guard, HttpRequest };



use std::{
    path::PathBuf,
    collections::HashMap,
};
use vod_to_podcast_rss::{transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec }, rss_transcodizer::RssTranscodizer};

async fn index() -> HttpResponse {
    HttpResponse::Ok().body("server works")
}
//TODO: should not be here, move in another module
async fn start_streaming_to_client() -> HttpResponse {
    println!("starting stream");
    let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path_to_mp3.push("src/transcoder/test.mp3");
    let stream_url = path_to_mp3.as_os_str().to_str().unwrap();
    println!("{stream_url}");
    let params = FfmpegParameters {
        seek_time: 0,
        url: stream_url.to_string(),
        max_rate_kbit: 64,
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: 64,
    };
    let transcoder = Transcoder::new(stream_url, &params);
    let stream = transcoder.get_transcode_stream();
    HttpResponse::Ok().content_type("audio/mpeg").no_chunking(327175).streaming(stream)
}

async fn transcodize_RSS(
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>
) -> HttpResponse {
    let url = if let Some(x) = query.get("url") {
        x
    } else {
        println!("no url provided");
        return HttpResponse::BadRequest().finish();
    };

        let transcode_service_url = req.url_for("transcode_mp3", &[""]).unwrap();

    //TODO cache with lifetime 1 minute
    let rss_transcodizer = RssTranscodizer::new(url.clone(), transcode_service_url);
    let body = match rss_transcodizer.transcodize().await {
        Ok(body) => body,
        Err(e) => {
            println!("could not translate rss");
            println!("{e}");
            return HttpResponse::Conflict().finish()
        },
    };
    HttpResponse::Ok().content_type("application/xml").body(body)
}

async fn transcode(query: web::Query<HashMap<String, String>>) -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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
            .route("/test_transcode.mp3", web::get().to(start_streaming_to_client))
            .route("/transcodize_rss", web::get().to(transcodize_RSS))
    })
        .bind(("127.0.0.1", 8080))?
        .run().await
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_example() {}
}