# Use the official Rust image as a base image
FROM rust:1.68 as builder

# Set the working directory
WORKDIR /usr/src/app

# Copy the source code and the Cargo.toml file
COPY src/ ./src/
COPY Cargo.toml Cargo.lock ./

# Install required system dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg clang libavformat-dev libavfilter-dev libavcodec-dev libavdevice-dev libavutil-dev libpostproc-dev libswresample-dev libswscale-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Build the Rust project
RUN cargo fetch
RUN cargo build --release

# Create a new stage with a minimal image
FROM debian:buster-slim

# Install required system dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg python3 curl && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp && \
    chmod a+rx /usr/local/bin/yt-dlp

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/app /usr/local/bin/vod_to_podcast
COPY --from=builder /lib /lib

# Expose the port used by the Rust application
EXPOSE 8080

# Start the Rust application
CMD ["vod_to_podcast"]
