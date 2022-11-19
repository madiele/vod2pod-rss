use actix_web::{ HttpServer, web, App, HttpResponse, guard, HttpRequest };
use reqwest::Url;
use rss::{ Channel, Enclosure };
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

    //TODO cache with lifetime 1 minute
    let rss_body = match (async { reqwest::get(url).await?.bytes().await }).await {
        Ok(x) => x,
        Err(e) => {
            println!("provided url gave an error: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    //RSS parsing
    let mut rss = match Channel::read_from(&rss_body[..]) {
        Ok(x) => x,
        Err(e) => {
            println!("could not parse rss, reason: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    //RSS translation
    for item in &mut rss.items {
        let enclosure = match &item.enclosure {
            Some(x) => x.clone(),
            None => {
                println!(
                    "item has no media, skipping: ({})",
                    serde_json::to_string(&item).unwrap_or_default()
                );
                continue;
            }
        };

        let media_url = match Url::parse(enclosure.url()) {
            Ok(x) => x,
            Err(_) => {
                println!("item has an invalid url");
                continue;
            }
        };

        let duration_str = item.itunes_ext
            .clone()
            .unwrap_or_default()
            .duration.unwrap_or("".to_string());

        //TODO use regex to extract duration from str '02:31:00'
        let duration_secs = match u32::from_str_radix(&duration_str, 10) {
            Ok(x) => x,
            Err(e) => {
                println!("failed to parse duration for {media_url}:\n{e}");
                continue;
            }
        };

        let mut transcode_url = req.url_for("transcode_mp3", &[""]).unwrap();
        
        transcode_url.query_pairs_mut().append_pair("url", media_url.as_str());

        //TODO set lenght and mime type
        item.set_enclosure(Enclosure {
            length: "0".to_string(),
            url: transcode_url.to_string(),
            mime_type: "".to_string(),
        });
    }

    HttpResponse::Ok().content_type("application/xml").body(rss.to_string())
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