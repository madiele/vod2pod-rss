use actix_web::{ HttpServer, web, App, HttpResponse, guard };
use rss::Channel;
use std::{ path::PathBuf, collections::HashMap };
use vod_to_podcast_rss::transcoder::{ Transcoder, FfmpegParameters, FFMPEGAudioCodec };

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

async fn transcodize_RSS(query: web::Query<HashMap<String, String>>) -> HttpResponse {
    let url = if let Some(x) = query.get("url") {
        x
    } else {
        println!("no url provided");
        return HttpResponse::BadRequest().finish();
    };

    //TODO cache with lifetime 1 minute
    let rss_body = match (async { reqwest::get(url).await?.bytes().await }).await {
        Ok(x) => x,
        Err(e) => {
            println!("provided url gave an error: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    //RSS parsing
    let rss = match Channel::read_from(&rss_body[..]) {
        Ok(x) => x,
        Err(e) => {
            println!("could not parse rss, reason: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    //RSS translation
    for item in rss.items {
        let media_url = match item.enclosure {
            Some(x) => x,
            None => {
                println!("item has no media, skipping: ({})", serde_json::to_string(&item).unwrap_or_default());
                continue;
            }
        };

        let duration_str = item.itunes_ext.unwrap_or_default().duration.unwrap_or("".to_string());
        //TODO if duration == "" then it must be calculated (pattern strategy?)
        //TODO parse duration
        
        
    }

    HttpResponse::Ok().finish()
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