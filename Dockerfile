# by using --platform=$BUILDPLATFORM we force the build step 
# to always run on the native architecture of the build machine
# making the build time shorter
FROM --platform=$BUILDPLATFORM rust:1.85 as builder

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
#trick to use github action cache, check the action folder for more info
RUN sed '/\[dev-dependencies\]/,/^$/d' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

RUN cargo fetch

COPY .cargo/ ./.cargo/
COPY src ./src
COPY set_version.sh version.txt* ./
COPY templates/ ./templates/

RUN sh set_version.sh

RUN cargo build --release --target "$(cat /rust_platform.txt)"

RUN echo "final size of vod2pod:\n $(ls -lah /tmp/vod2pod/target/*/release/app)"

#-------------
# Build Deno for target architecture using cargo
# This stage runs on the build platform but compiles for the target platform
FROM --platform=$BUILDPLATFORM rust:1.90 as deno-builder

ARG BUILDPLATFORM
ARG TARGETPLATFORM

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

RUN echo "Deno build stage running on $BUILDPLATFORM, building for $TARGETPLATFORM (rust target $(cat /rust_platform.txt))"

RUN if echo $TARGETPLATFORM | grep -q 'arm'; then \
        apt-get update && apt-get install -y build-essential gcc gcc-arm* gcc-aarch* && apt-get clean; \
    fi

RUN rustup target add $(cat /rust_platform.txt)

# Use cargo to install deno into /deno for the selected target. This attempts to cross-compile
# and will require the appropriate cross toolchain when targeting different architectures.
RUN cargo install --locked deno@2.6.0 --root /deno --target "$(cat /rust_platform.txt)" || true

RUN if [ -x /deno/bin/deno ]; then echo "deno built at /deno/bin/deno"; else echo "deno not available"; fi

#----------
#this step will always run on the target architecture,
#so the build driver will need to be able to support runtime commands on it (es: using QEMU)  
FROM --platform=$TARGETPLATFORM debian:bookworm-slim as app

ARG BUILDPLATFORM
ARG TARGETPLATFORM

RUN echo "I am running on $BUILDPLATFORM, building for $TARGETPLATFORM"
COPY requirements.txt ./
#install ffmpeg and yt-dlp
RUN apt-get update && \
    apt-get install -y --no-install-recommends unzip python3 curl ca-certificates ffmpeg && \
    export YT_DLP_VERSION=$(cat requirements.txt | grep yt-dlp | cut -d "=" -f3 | awk -F. '{printf "%d.%02d.%02d\n", $1, $2, $3}') && \
    curl -L https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/yt-dlp -o /usr/local/bin/yt-dlp && \
    chmod a+rx /usr/local/bin/yt-dlp && \
    curl -fsSL https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip -o deno.zip && \
    unzip deno.zip -d /usr/local/bin && \
    rm deno.zip && \
    apt-get -y purge curl && \
    apt-get -y purge unzip && \
    apt-get -y autoremove && \
    apt-get -y clean && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/vod2pod/target/*/release/app /usr/local/bin/vod2pod
COPY --from=builder /tmp/vod2pod/templates/ ./templates
COPY --from=deno-builder /deno/bin/deno /usr/local/bin/deno
#
RUN if deno --version; then \
        echo "deno runs correctly"; \
    else \
        echo "deno did not run" 1>&2; \
        exit 1; \
    fi

RUN if vod2pod --version; then \
        echo "vod2pod starts correctly"; \
        exit 0; \
    else \
        echo "vod2pod did not start" 1>&2; \
        exit 1; \
    fi

EXPOSE 8080

CMD ["vod2pod"]
