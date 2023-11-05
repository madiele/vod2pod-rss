# this step always runs nativly
FROM --platform=$BUILDPLATFORM rust:1.73 as builder

ARG BUILDPLATFORM
ARG TARGETPLATFORM
ARG RUST_TARGET_PLATFORM

# Referred TARGETPLATFORM to appropriate Rust platform
RUN if [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then \
        export RUST_TARGET_PLATFORM=armv7-unknown-linux-gnueabihf; \
    elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        export RUST_TARGET_PLATFORM=aarch64-unknown-linux-gnu; \
    elif [ "$TARGETPLATFORM" = "linux/amd64" ]; then \
        export RUST_TARGET_PLATFORM=x86_64-unknown-linux-gnu; \
    else \
        export RUST_TARGET_PLATFORM=$(rustup target list --installed | head -n 1); \
    fi; \
    echo "choosen rust target: $RUST_TARGET_PLATFORM" ;\
    echo $RUST_TARGET_PLATFORM > rust_platform.txt

run ls
run cat rust_platform.txt
RUN echo "I am running on $BUILDPLATFORM, building for $TARGETPLATFORM, rust target is $(cat rust_platform.txt)"

RUN rustup target add $(cat rust_platform.txt) 
RUN rustup toolchain install $(cat rust_platform.txt) 

RUN cd /tmp && USER=root cargo new --bin vod2pod
WORKDIR /tmp/vod2pod
COPY Cargo.toml ./
RUN sed '/\[dev-dependencies\]/,/^$/d' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

RUN cargo fetch
RUN cargo install cargo-build-deps

#RUN apt-get update && \
#    apt-get install -y --no-install-recommends ffmpeg clang libavformat-dev libavfilter-dev libavcodec-dev libavdevice-dev libavutil-dev libpostproc-dev libswresample-dev libswscale-dev && \
#    apt-get clean && \
#    rm -rf /var/lib/apt/lists/*

#workaround for https://github.com/rust-lang/cargo/issues/8719
#ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

RUN cargo build-deps --release --target $(cat rust_platform.txt)
COPY src /tmp/vod2pod/src

#trick to use github action cache, check the action folder for more info
COPY set_version.sh version.txt* ./
COPY templates/ ./templates/
RUN sh set_version.sh

RUN cargo build --release --target $(cat rust_platform.txt)

#----------
FROM debian:bullseye-slim

#install ffmpeg and yt-dlp
ARG BUILDPLATFORM
ARG TARGETPLATFORM

RUN echo "I am running on $BUILDPLATFORM, building for $TARGETPLATFORM"
COPY requirements.txt ./
RUN apt-get update && \
    apt-get install -y --no-install-recommends python3 curl ca-certificates ffmpeg && \
    export YT_DLP_VERSION=$(cat requirements.txt | grep yt-dlp | cut -d "=" -f3 | awk -F. '{printf "%d.%02d.%02d\n", $1, $2, $3}') && \
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
