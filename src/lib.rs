pub mod transcoder;
pub mod rss_transcodizer;
pub mod feed_url;

use actix_web::dev::Server;
use actix_web::HttpResponse;
use actix_web::{ web, App, HttpServer };
use std::net::TcpListener;

async fn health() -> HttpResponse {
    HttpResponse::Ok().finish()
}

pub fn run(listener: TcpListener) -> Result<Server, std::io::Error> {
    let server = HttpServer::new(|| App::new().route("/health", web::get().to(health)))
        .listen(listener)?
        .run();
    // No .await here!
    Ok(server)
}
