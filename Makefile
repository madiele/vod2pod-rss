#!make
define write-env =
touch .env
grep -q '^TRANSCODE=' .env || echo 'TRANSCODE=true' >> .env
grep -q '^YT_API_KEY=' .env || echo 'YT_API_KEY=' >> .env
grep -q '^TWITCH_SECRET=' .env || echo 'TWITCH_SECRET=' >> .env
grep -q '^TWITCH_CLIENT_ID=' .env || echo 'TWITCH_CLIENT_ID=' >> .env
grep -q '^RUST_LOG=' .env || echo 'RUST_LOG=DEBUG' >> .env
grep -q '^MP3_BITRATE=' .env || echo 'MP3_BITRATE=192' >> .env
grep -q '^SUBFOLDER=' .env || echo 'SUBFOLDER=/' >> .env
grep -q '^TWITCH_TO_PODCAST_URL=' .env || echo 'TWITCH_TO_PODCAST_URL=localhost:8085' >> .env
grep -q '^PODTUBE_URL=' .env || echo 'PODTUBE_URL=http://localhost:15000' >> .env
grep -q '^REDIS_ADDRESS=' .env || echo 'REDIS_ADDRESS=localhost' >> .env
grep -q '^REDIS_PORT=' .env || echo 'REDIS_PORT=6379' >> .env
endef

ifneq (,$(wildcard ./.env))
    include .env
    export
endif

install-ubuntu-deps:
	$(write-env)
	sudo apt update
	sudo apt install -y ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	source "$${HOME}}/.cargo/env"
	cargo install cargo-watch
	pip3 install yt-dlp --yes

install-fedora-deps:
	$(write-env)
	sudo dnf update
	sudo dnf install -y ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	source "$${HOME}}/.cargo/env"
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
