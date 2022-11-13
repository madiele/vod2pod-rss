use actix_web::{ HttpServer, web, App, HttpResponse };
use std::{ path::PathBuf };
use vod_to_podcast_rss::{ transcoder::{ Transcoder, FFMPEG_parameters, FFMPEGAudioCodec } };

async fn index() -> HttpResponse {
    HttpResponse::Ok().body("server works")
}
//TODO: should not be here, move in another module
async fn start_streaming_to_client() -> HttpResponse {
    println!("starting stream");
    let mut path_to_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path_to_mp3.push("src/transcoder/test.mp3");
    let duration = 27;
    let stream_url = path_to_mp3.as_os_str().to_str().unwrap();
    println!("{stream_url}");
    let params = FFMPEG_parameters {
        seek_time: 0,
        url: stream_url.to_string(),
        max_rate_kbit: 64,
        audio_codec: FFMPEGAudioCodec::Libmp3lame,
        bitrate_kbit: 64,
    };
    let transcoder = Transcoder::new(duration, stream_url, &params);
    let stream = transcoder.get_transcode_stream();
    HttpResponse::Ok().content_type("audio/mpeg").no_chunking(327175).streaming(stream)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .route("/test_transcode.mp3", web::get().to(start_streaming_to_client))
    })
        .bind(("127.0.0.1", 8080))?
        .run().await
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_example() {}
}