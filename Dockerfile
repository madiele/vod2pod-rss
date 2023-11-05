# by using --platform=$BUILDPLATFORM we force the build step 
# to always run on the native architecture of the build machine
# making the build time shorter
FROM --platform=$BUILDPLATFORM rust:1.73 as builder

ARG BUILDPLATFORM
ARG TARGETPLATFORM

#find the right build target for rust
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
    echo $RUST_TARGET_PLATFORM > /rust_platform.txt

RUN echo "I am running on $BUILDPLATFORM, building for $TARGETPLATFORM, rust target is $(cat /rust_platform.txt)"

RUN if echo $TARGETPLATFORM | grep -q 'arm'; then \
        echo 'Installing packages for ARM platforms...'; \
        apt-get update && apt-get install  build-essential gcc gcc-arm* gcc-aarch* -y && apt-get clean; \
        echo 'gcc-arm* packages installed and cache cleaned.'; \
    fi

RUN rustup target add $(cat /rust_platform.txt) 

RUN cd /tmp && USER=root cargo new --bin vod2pod

WORKDIR /tmp/vod2pod

COPY Cargo.toml ./
RUN sed '/\[dev-dependencies\]/,/^$/d' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

RUN cargo fetch
COPY . /tmp/vod2pod

RUN sh set_version.sh

RUN echo "final size of vod2pod:\n $(du -sh /tmp/vod2pod/target/*/release/app)"

RUN cargo build --release --target "$(cat /rust_platform.txt)"

#----------
FROM debian:bookworm-slim as app

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

COPY --from=builder /tmp/vod2pod/target/*/release/app /usr/local/bin/vod2pod
COPY --from=builder /tmp/vod2pod/templates/ ./templates

RUN if vod2pod --version; then \
        echo "vod2pod starts correctly"; \
        exit 0; \
    else \
        echo "vod2pod did not start" 1>&2; \
        exit 1; \
    fi

EXPOSE 8080

CMD ["vod2pod"]
