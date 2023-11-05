use configs::{conf, Conf, ConfName};

pub mod configs;
pub mod provider;
pub mod rss_transcodizer;
pub mod server;
pub mod transcoder;

pub async fn get_redis_client() -> Result<redis::aio::Connection, eyre::Error> {
    let redis_address = conf().get(ConfName::RedisAddress).unwrap();
    let redis_port = conf().get(ConfName::RedisPort).unwrap();
    let client = redis::Client::open(format!("redis://{}:{}/", redis_address, redis_port))?;
    let con = client.get_tokio_connection().await?;
    Ok(con)
}

