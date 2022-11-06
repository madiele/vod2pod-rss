use dotenv::dotenv;
use log::info;
use std::net::TcpListener;
use VoDToPodcastRSS::run;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    pretty_env_logger::init();

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    info!("Starting app...");

    run(listener)?.await
}
