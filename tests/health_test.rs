use std::net::TcpListener;

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

// Launch our application in the background
fn spawn_app() -> String {
  let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
  let port = listener.local_addr().unwrap().port();
  let server = vod2pod_rss::run(listener).expect("Failed to bind address");
  let _ = tokio::spawn(server);

  format!("http://127.0.0.1:{}", port)
}
