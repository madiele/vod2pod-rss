#!make
define write-env =
echo TRANSCODE=true > .dev.env
echo YT_API_KEY= >> .dev.env
echo TWITCH_SECRET= >> .dev.env
echo TWITCH_CLIENT_ID= >> .dev.env
echo RUST_LOG=DEBUG >> .dev.env
echo MP3_BITRATE=192 >> .dev.env
echo SUBFOLDER=/ >> .dev.env
echo TWITCH_TO_PODCAST_URL=localhost:8085 >> .dev.env
echo PODTUBE_URL=http://localhost:15000 >> .dev.env
echo REDIS_ADDRESS=redis >> .dev.env
echo REDIS_PORT=6379 >> .dev.env
endef

ifneq (,$(wildcard ./.dev.env))
    include .dev.env
    export
endif

install-ubuntu-deps:
	$(write-env)
	sudo apt update
	sudo apt install -y ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	cargo install cargo-watch
	pip3 install yt-dlp --yes

install-fedora-deps:
	$(write-env)
	sudo dnf update
	sudo dnf install -y ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	cargo install cargo-watch
	pip3 install yt-dlp --yes

start-deps:
	@if which docker-compose >/dev/null; then \
		sudo docker-compose -f docker-compose.dev_enviroment.yml up --force-recreate -d; \
	else \
		sudo docker compose -f docker-compose.dev_enviroment.yml up --force-recreate -d; \
	fi

image:
	@if which docker-compose >/dev/null; then \
		sudo docker-compose build; \
	else \
		sudo docker compose build; \
	fi

test:
	cargo test -- --nocapture

test-watch:
	cargo watch "test -- --nocapture"

run:
	redis-cli flushdb
	cargo run

hot-reload:
	cargo watch -s "redis-cli flushdb && cargo run"
