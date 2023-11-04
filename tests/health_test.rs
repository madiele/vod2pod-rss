use std::net::TcpListener;
use vod2pod_rss;

#[actix_rt::test]
async fn health_works() {
    // Arrange
    let address = spawn_app();
    let client = reqwest::Client::new();

    // Act
    let response = client
        .get(&format!("{}/health", &address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

#[actix_rt::test]
async fn fetch_yt_feed_by_channel_url_ok() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let server = vod2pod_rss::server::spawn_server(listener).expect("Failed to bind address");
    let _ = tokio::spawn(server);
    let client = reqwest::Client::new();

    let response = client
        .get(&format!(
            "{}/transcodize_rss",
            &format!("http://127.0.0.1:{}", port)
        ))
        .query(&[("url", "https://www.youtube.com/@LinusTechTips")])
        .send()
        .await
        .expect("Failed to execute request.");

    println!("status: {:?}", response.status());

    println!(
        "response: {:?}",
        response.text().await.expect("failed to get response text")
    )
}

#[actix_rt::test]
async fn fetch_yt_feed_by_video_url_fail() {}

#[actix_rt::test]
async fn fetch_yt_feed_by_playlist_url_ok() {}

#[actix_rt::test]
async fn fetch_twitch_feed_by_channel_url_ok() {}

// Launch our application in the background
fn spawn_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let server = vod2pod_rss::run(listener).expect("Failed to bind address");
    let _ = tokio::spawn(server);

    format!("http://127.0.0.1:{}", port)
}

