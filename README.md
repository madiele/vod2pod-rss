# vod2pod-rss [![tests](https://github.com/madiele/vod2pod-rss/actions/workflows/rust.yml/badge.svg)](https://github.com/madiele/vod2pod-rss/actions/workflows/rust.yml) [![stable image](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image.yml/badge.svg?branch=stable)](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image.yml) [![beta image](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image-beta.yml/badge.svg)](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image-beta.yml)

Converts a YouTube or Twitch channel into a full blown audio podcast feed.

<a label="example of it working with podcast addict" href="https://user-images.githubusercontent.com/4585690/231301791-2f838fb3-4f6e-4382-bac4-c968bfe98c08.png"><img src="https://user-images.githubusercontent.com/4585690/231301791-2f838fb3-4f6e-4382-bac4-c968bfe98c08.png" align="right" height="350" ></a>

## Features

- Completely converts the VoDs into a proper podcast RSS that can be listened to directly inside the client.
- The VoDs are not downloaded on the server, so no need for storage while self-hosting this app.
- VoDs are transcoded to MP3 192k on the fly by default, tested to be working flawlessly even on a Raspberry Pi 3-4.
- also works on standard rss podcasts feed if you want to have a lower bitrate version to save mobile data.

## Limitations

- Youtube channel avatar is not present and results are limited to 15 when no youtube API key is set

# Usage

<a label="frontend" href="https://user-images.githubusercontent.com/4585690/234704870-0bf3023a-78e0-4ccc-adea-9d1f6ea2fabc.png"><img src="https://user-images.githubusercontent.com/4585690/234704870-0bf3023a-78e0-4ccc-adea-9d1f6ea2fabc.png" align="right" width="400px" ></a>
## Generate Podcast RSS
- In a web browser goto: http://myserver.com/ or http://localhost/
  - In the web page that opens paste the channel you want to convert to podcast and copy the generated link.
- Optionally goto : http://myserver.com/`/transcodize_rss?url=channel_url`
  - An RSS will be generated.
  - Replace `channel_url` with the URL of the YouTube or Twitch channel you want to convert into a podcast.

### Examples
- YouTube: `myserver.com/transcodize_rss?url=https://www.youtube.com/c/channelname`
- Twitch: `myserver.com/transcodize_rss?url=https://www.twitch.tv/channelname`
- RSS/atom feed: `myserver.com/transcodize_rss?url=https://feeds.simplecast.com/aU_RzZ7j`
  - Add the domain to the whitelist (see configuration below)

## Podcast Client
Add the RSS link to your podcast client.
- <https://transistor.fm/add-podcast/>

# Install
## Optional API Access
- Twitch: Get your SECRET and CLIENT ID <https://dev.twitch.tv/console>
- YouTube: More than 15 items in the RSS feed, channel avatar
  - API key <https://developers.google.com/youtube/v3/getting-started>
  - Enable API Access <https://console.cloud.google.com/>
    - APIs & Services > +Enable APIs and Services > Search "YouTube Data API"

## Docker
- Precompiled images are available [here](https://hub.docker.com/r/madiele/vod2pod-rss/) for linux machines with arm64, amd64 and armv7 (raspberry pis are supported).
- Mac: Make sure Rust is installed
  ```
  brew info rust
  ```
  ```
  brew install rust
  ``` 

### [Docker Compose](https://docs.docker.com/compose/install/)

```
git clone https://github.com/madiele/vod2pod-rss.git
```

```
cd vod2pod-rss
```

**docker-compose.yml**
```
nano docker-compose.yml
```
```
sudo docker compose up -d
```

### Updating

`sudo docker compose pull && sudo docker compose up -d`

then run this to delete the old version from your system (note: this will also delete any other unused image you have)

`sudo docker system prune`
#### new releases notifications

To get notifications of new release be sure to watch the repo and set a custom notification only on new releases.



## Configuration
**Web Server Port**
- `ports`: "80:8080" #Change 80 to another port if you already use the port 80 on your host

**Optional API Keys**
- `YT_API_KEY`: Set your YouTube API key (works without but the feed is limited to 15)
  - e.g. YT_API_KEY=AIzaSyBTCCEOHm
- `TWITCH_SECRET`: Set your Twitch secret
- `TWITCH_CLIENT_ID`: Set your Twitch client ID
  Note: These can also be set using Docker [.env files](https://docs.docker.com/compose/environment-variables/env-file/) 

**Environment**
- `TRANSCODE`: Set to "false" to disable transcoding, usefull if you only need the feeds (default: "true")
- `MP3_BITRATE`: Set the bitrate of the trascoded stream to your client (default: "192")
- `SUBFOLDER`: Set the the root path of the app, useful for reverse proxies (default: "/")
- `VALID_URL_DOMAINS`: (optional) Set a comma separated list of domains urls that are allowed to be converted into RSS  (defaults to yotube and twitch urls)

## Honorable mentions

These projects were fundamental for the success of vod2pod-rss, originally they handled the feed generation for youtube and twitch, now this is all done by vod2pod-rss internally so they are not used anymore, but were still helpful to get vod2pod-rss up and running fast.
* Youtube support was possible thanks to the cool [podtube fork project by amckee](https://github.com/amckee/PodTube) consider dropping him a star.
* Twitch support was possible thanks to [my fork](https://github.com/madiele/TwitchToPodcastRSS) of [lzeke0's TwitchRSS](https://github.com/lzeke0/TwitchRSS) drop a star to him too!

## Donations

This is a passion project, and mostly made for personal use, but if you want to gift a pizza margherita, feel free!

[!["Buy Me A Coffee"](https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png)](https://www.buymeacoffee.com/madiele)

## Contributing

check the [CONTRIBUTING.md](CONTRIBUTING.md) to find a tutorial on how to setup your enviroment for develpment
