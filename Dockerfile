FROM rust:1.70 as builder

RUN cd /tmp && USER=root cargo new --bin vod2pod
WORKDIR /tmp/vod2pod
COPY Cargo.toml ./
RUN sed '/\[dev-dependencies\]/,/^$/d' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

RUN cargo fetch
RUN cargo install cargo-build-deps

RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg clang libavformat-dev libavfilter-dev libavcodec-dev libavdevice-dev libavutil-dev libpostproc-dev libswresample-dev libswscale-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN cargo build-deps --release
COPY src /tmp/vod2pod/src

#trick to use github action cache, check the action folder for more info
COPY set_version.sh version.txt* ./
COPY templates/ ./templates/
RUN sh set_version.sh

RUN cargo build --release

#----------
FROM debian:bullseye-slim

#install ffmpeg and yt-dlp
ARG TARGETPLATFORM
COPY requirements.txt ./
RUN apt-get update && \
    apt-get install -y --no-install-recommends python3 curl ca-certificates ffmpeg && \
    export YT_DLP_VERSION=$(cat requirements.txt | grep yt-dlp | cut -d "=" -f3) && \
    curl -L https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/yt-dlp -o /usr/local/bin/yt-dlp && \
    chmod a+rx /usr/local/bin/yt-dlp && \
    apt-get -y purge curl && \
    apt-get -y autoremove && \
    apt-get -y clean && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/vod2pod/target/release/app /usr/local/bin/vod2pod
COPY --from=builder /tmp/vod2pod/templates/ ./templates


EXPOSE 8080

CMD ["vod2pod"]
