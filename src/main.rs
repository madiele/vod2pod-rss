use log::info;
use std::net::TcpListener;
use vod_to_podcast_rss::run;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    info!("Starting app...");

    run(listener)?.await
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_example() {
        
    }
}