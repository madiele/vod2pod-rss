# vod2pod-rss [![tests](https://github.com/madiele/vod2pod-rss/actions/workflows/rust.yml/badge.svg)](https://github.com/madiele/vod2pod-rss/actions/workflows/rust.yml) [![stable image](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image.yml/badge.svg?branch=stable)](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image.yml) [![beta image](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image-beta.yml/badge.svg)](https://github.com/madiele/vod2pod-rss/actions/workflows/docker-image-beta.yml)

Converts a YouTube or Twitch channel into a full blown audio podcast feed.

<a label="example of it working with podcast addict" href="https://user-images.githubusercontent.com/4585690/231301791-2f838fb3-4f6e-4382-bac4-c968bfe98c08.png"><img src="https://user-images.githubusercontent.com/4585690/231301791-2f838fb3-4f6e-4382-bac4-c968bfe98c08.png" align="right" height="350" ></a>

# Features
- Completely converts the VoDs into a proper podcast RSS that can be listened to directly inside the client.
- The VoDs are not downloaded on the server, so no need for storage while self-hosting this app.
- VoDs are transcoded to MP3 192k on the fly by default, tested to be working flawlessly even on a Raspberry Pi 3-4.
- also works on standard rss podcasts feed if you want to have a lower bitrate version to save mobile data.

## Limitations
- Youtube channel avatar is not present and results are limited to 15 when no YouTube API key is set.

# Usage

## Web UI
<a label="frontend" href="https://user-images.githubusercontent.com/4585690/234704870-0bf3023a-78e0-4ccc-adea-9d1f6ea2fabc.png"><img src="https://user-images.githubusercontent.com/4585690/234704870-0bf3023a-78e0-4ccc-adea-9d1f6ea2fabc.png" align="right" width="400px" ></a>
- just go where you hosted vod2pod and you will find an easy to use UI to generate the feed
## Manually Generate A Podcast URL
- In a web browser go to where you hosted vod2pod, es: http://myserver.com/ or http://localhost/
  - In the web page that opens paste the channel you want to convert to podcast and copy the generated link.
- Optionally goto : http://myserver.com/transcodize_rss?url=channel_url
  - An RSS will be generated.
  - Replace `channel_url` with the URL of the YouTube or Twitch channel you want to convert into a podcast.
    - YouTube: `http://myserver.com/transcodize_rss?url=https://www.youtube.com/c/channelname`
    - Twitch: `http://myserver.com/transcodize_rss?url=https://www.twitch.tv/channelname`
    - RSS/atom feed: `http://myserver.com/transcodize_rss?url=https://feeds.simplecast.com/aU_RzZ7j`
      - Add the domain to the whitelist. See configurations [below](#configurations)

## Add The URL To A Podcast Client
- find a tutorial on how to add an rss feed to your favorite podcast app

# Optional API Access
- Twitch: Get your SECRET and CLIENT ID <https://dev.twitch.tv/console>
- YouTube: Enable more than 15 items in the RSS feed, channel avatar
  - API key <https://developers.google.com/youtube/v3/getting-started>
  - Enable API Access <https://console.cloud.google.com/>
    - APIs & Services > +Enable APIs and Services > Search "YouTube Data API"

See configurations [below](#configurations)

# Install
## Clone This Repository  
```
git clone https://github.com/madiele/vod2pod-rss.git
```

## Docker
- Install [Docker Compose](https://docs.docker.com/compose/install/)
- Precompiled images are available [here](https://hub.docker.com/r/madiele/vod2pod-rss/) for linux machines with arm64, amd64 and armv7 (raspberry pis are supported).

### Docker Compose
```
cd vod2pod-rss
nano docker-compose.yml
```
See configurations [below](#configurations)
```
sudo docker compose up -d
```

#### Updating
```
sudo docker compose pull && sudo docker compose up -d
sudo docker system prune
```
- To get notifications of new release follow [these instructions](https://docs.github.com/en/account-and-profile/managing-subscriptions-and-notifications-on-github/setting-up-notifications/about-notifications)

#### Switching to the Beta branch

The beta branch is a version of vod2pod that is always updated to the latest yt-dlp releases in a matter of days, if you have problems try it out first to see if they are fixed, then open an issue so that I can consider making a new stable release

Also by being on the beta branch you might help me find bugs before I make any new stable release, so you'll help the project too

To switch open the compose docker-compose.yml and edit the vod2pod image section from "latest" to "beta", then follow the steps to update

## Configurations
### Web Server Port
- `ports`: "80:8080" (optional) Change 80 to another port if you already use the port 80 on your host
  - e.g. "81:8080" http://myserver.com:81/

### Optional API Keys
- `YT_API_KEY`: Set your YouTube API key (works without but the feed is limited to 15)
  - e.g. YT_API_KEY=AIzaSyBTCCEOHm
- `PROXY`: Set a proxy for yt-dlp requests (optional)
  - e.g. PROXY=socks5://127.0.0.1:1080 or PROXY=http://proxy.example.com:8080
- `TWITCH_SECRET`: Set your Twitch secret
- `TWITCH_CLIENT_ID`: Set your Twitch client ID

Note: These can also be set using Docker [.env files](https://docs.docker.com/compose/environment-variables/env-file/) 

### Environment
- `TRANSCODE`: Set to "false" to disable transcoding, usefull if you only need the feeds (default: "true")
- `MP3_BITRATE`: Set the bitrate of the trascoded stream to your client (default: "192")
- `SUBFOLDER`: Set the the root path of the app, useful for reverse proxies (default: "/")
- `VALID_URL_DOMAINS`: (optional) Set a comma separated list of domain urls that are allowed to be converted into RSS  (defaults to YouTube and Twitch urls)
- `CACHE_TTL`: (optional) Set the time to live of the cache in seconds, default is 600 seconds (10 minutes)

# Honorable Mentions

These projects were fundamental for the success of vod2pod-rss, originally they handled the feed generation for youtube and twitch, now this is all done by vod2pod-rss internally so they are not used anymore, but were still helpful to get vod2pod-rss up and running fast.
* Youtube support was possible thanks to the cool [podtube fork project by amckee](https://github.com/amckee/PodTube) consider dropping him a star.
* Twitch support was possible thanks to [my fork](https://github.com/madiele/TwitchToPodcastRSS) of [lzeke0's TwitchRSS](https://github.com/lzeke0/TwitchRSS) drop a star to him too!

## Donations

This is a passion project, and mostly made for personal use, but if you want to gift a pizza margherita, feel free!

[!["Buy Me A Coffee"](https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png)](https://www.buymeacoffee.com/madiele)

## Contributing

check the [CONTRIBUTING.md](CONTRIBUTING.md) to find a tutorial on how to setup your enviroment for develpment
