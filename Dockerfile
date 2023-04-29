FROM rust:1.68 as builder

RUN cd /tmp && USER=root cargo new --bin vod2pod
WORKDIR /tmp/vod2pod
COPY Cargo.toml ./

RUN cargo fetch
RUN cargo install cargo-build-deps

RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg clang libavformat-dev libavfilter-dev libavcodec-dev libavdevice-dev libavutil-dev libpostproc-dev libswresample-dev libswscale-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

#VERSION=$(grep -oP '^version = "\K[0-9]+\.[0-9]+\.[0-9]+' $TOML_FILE)
#sed '/\[dev-dependencies\]/,/^$/d' $TOML_FILE | sed 's/^version = .*$/version = "0\.0\.1"/' > "$TMP_FILE"
#echo "$VERSION" > version.txt

RUN cargo build-deps --release
COPY src /tmp/vod2pod/src
COPY set_version.sh ./
RUN sh set_version.sh

RUN cargo build  --release

#----------
FROM debian:bullseye-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg python3 curl libpcre2-dev ca-certificates && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp && \
    chmod a+rx /usr/local/bin/yt-dlp

COPY --from=builder /tmp/vod2pod/target/release/app /usr/local/bin/vod2pod

COPY templates/ ./templates/

EXPOSE 8080

CMD ["vod2pod"]
