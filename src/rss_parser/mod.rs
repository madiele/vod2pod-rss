//this module will take an existing RSS and output a new RSS with the enclosure replaced by the trascode URL
//usage example
//GET /rss?url=https://website.com/url/to/feed.rss
//media will have original url replaced by
//GET /transcode/UUID?url=https://website.com/url/to/media.mp3
//GET /transcode/UUID?url=https://website.com/url/to/media.mp4
//GET /transcode/UUID?url=https://website.com/url/to/media.m3u8 (this will require some trickery to get the correct duration)