use rss::Channel;
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
    let address = spawn_app();
    let client = reqwest::Client::new();

    let response = client
        .get(&format!("{}/transcodize_rss", &address))
        .query(&[("url", "https://www.youtube.com/@madiele92")])
        .send()
        .await
        .expect("Failed to execute request.");

    println!("status: {:?}", response.status());

    if !response.status().is_success() {
        println!("response body: {:?}", response.text().await);
        panic!("response status is not success");
    }

    let raw_rss_body = response.text().await.expect("failed to get response text");
    let feed = rss::Channel::read_from(&raw_rss_body.as_bytes()[..]).expect("failed to parse feed");
    println!("{:?}", feed);
    check_youtube_feed(feed);
}

#[actix_rt::test]
async fn fetch_yt_feed_by_channel_id_url_ok() {
    let address = spawn_app();
    let client = reqwest::Client::new();

    let response = client
        .get(&format!("{}/transcodize_rss", &address))
        .query(&[(
            "url",
            "https://www.youtube.com/channel/UCXssEBQ8JWH1NacVIyQXe8g",
        )])
        .send()
        .await
        .expect("Failed to execute request.");

    println!("status: {:?}", response.status());

    if !response.status().is_success() {
        println!("response body: {:?}", response.text().await);
        panic!("response status is not success");
    }

    let raw_rss_body = response.text().await.expect("failed to get response text");
    let feed = rss::Channel::read_from(&raw_rss_body.as_bytes()[..]).expect("failed to parse feed");
    println!("{:?}", feed);
    check_youtube_feed(feed);
}

#[actix_rt::test]
async fn fetch_yt_feed_by_playlist_url_ok() {
    let address = spawn_app();
    let client = reqwest::Client::new();

    let response = client
        .get(&format!("{}/transcodize_rss", &address))
        .query(&[(
            "url",
            "https://www.youtube.com/playlist?list=PL589F357911E267F7",
        )])
        .send()
        .await
        .expect("Failed to execute request.");

    println!("status: {:?}", response.status());

    if !response.status().is_success() {
        println!("response body: {:?}", response.text().await);
        panic!("response status is not success");
    }

    let raw_rss_body = response.text().await.expect("failed to get response text");
    let feed = rss::Channel::read_from(&raw_rss_body.as_bytes()[..]).expect("failed to parse feed");
    println!("{:?}", feed);
    assert!(feed.items.len() > 0);
    assert!(feed.image().is_some());
    for item in feed.items() {
        assert!(item.link().is_some());
        assert!(item.itunes_ext().is_some());
        assert!(item.itunes_ext().unwrap().image().is_some());
    }
}

#[actix_rt::test]
async fn fetch_twitch_feed_by_channel_url_ok() {
    let address = spawn_app();
    let client = reqwest::Client::new();

    let response = client
        .get(&format!("{}/transcodize_rss", &address))
        .query(&[("url", "https://www.twitch.tv/twitch")])
        .send()
        .await
        .expect("Failed to execute request.");

    println!("status: {:?}", response.status());

    if !response.status().is_success() {
        println!("response body: {:?}", response.text().await);
        panic!("response status is not success");
    }

    let raw_rss_body = response.text().await.expect("failed to get response text");
    let feed = rss::Channel::read_from(&raw_rss_body.as_bytes()[..]).expect("failed to parse feed");
    println!("{:?}", feed);
    assert!(feed.items.len() > 0);
    assert!(feed.image().is_some());
    for item in feed.items() {
        assert!(item.link().is_some());
        assert!(item.itunes_ext().is_some());
        assert!(item.itunes_ext().unwrap().image().is_some());
    }
}

fn check_youtube_feed(feed: Channel) {
    assert_eq!("Mattia Di Eleuterio", feed.title());
    assert_eq!(
        "https://www.youtube.com/channel/UCXssEBQ8JWH1NacVIyQXe8g",
        feed.link()
    );
    assert_eq!(".", feed.description());
    assert!(feed.image().is_some());
    assert!(feed.items.len() > 0);
    let mut found = 0;
    for item in feed.items() {
        match item.title() {
            Some("Spin rhythm psmove demo") => {
                let itunes = item.itunes_ext().unwrap();
                assert_eq!(itunes.duration(), Some("00:02:37"));
                let image_url = itunes.image().unwrap();
                let url_format =
                    regex::Regex::new(r"^https://i\.ytimg\.com/vi/[a-zA-Z0-9_-]*/.*\.jpg$")
                        .unwrap();
                assert!(url_format.is_match(&image_url));
                found += 1;
            }
            Some("effetto disegno con gimp") => {
                let itunes = item.itunes_ext().unwrap();
                let image_url = itunes.image().unwrap();
                let url_format =
                    regex::Regex::new(r"^https://i\.ytimg\.com/vi/[a-zA-Z0-9_-]*/.*\.jpg$")
                        .unwrap();
                assert!(url_format.is_match(&image_url));
                assert_eq!(itunes.duration(), Some("00:09:03"));
                found += 1;
            }
            Some("il terremoto del 7/04 alle 19:43 ripreso in diretta") => {
                let itunes = item.itunes_ext().unwrap();
                let image_url = itunes.image().unwrap();
                let url_format =
                    regex::Regex::new(r"^https://i\.ytimg\.com/vi/[a-zA-Z0-9_-]*/.*\.jpg$")
                        .unwrap();
                assert!(url_format.is_match(&image_url));
                assert_eq!(itunes.duration(), Some("00:02:07"));
                found += 1;
            }
            Some("tutorial Burning words with gimp / scritte di fuoco con gimp") => {
                let itunes = item.itunes_ext().unwrap();
                let image_url = itunes.image().unwrap();
                let url_format =
                    regex::Regex::new(r"^https://i\.ytimg\.com/vi/[a-zA-Z0-9_-]*/.*\.jpg$")
                        .unwrap();
                assert!(url_format.is_match(&image_url));
                assert_eq!(itunes.duration(), Some("00:04:14"));
                found += 1;
            }
            Some(_) => {
                assert!(item.link().is_some());
                found += 1;
            }
            None => panic!("item has no title:\n{:?}", item),
        }
    }
    assert!(found > 3);
}

// Launch our application in the background
fn spawn_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let server = vod2pod_rss::server::spawn_server(listener).expect("Failed to bind address");
    let _ = tokio::spawn(server);

    format!("http://127.0.0.1:{}", port)
}

