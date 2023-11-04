pub mod configs;
pub mod provider;
pub mod rss_transcodizer;
pub mod server;
pub mod transcoder;

use actix_web::dev::Server;
use actix_web::{guard, middleware, HttpResponse};
use actix_web::{web, App, HttpServer};
use configs::{conf, ConfName};
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

