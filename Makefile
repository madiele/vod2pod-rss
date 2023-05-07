#!make
ifneq (,$(wildcard ./.dev.env))
    include .dev.env
    export
endif

install-ubuntu-deps:
	sudo apt update
	sudo apt install -y ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	cargo install cargo-watch
	pip3 install yt-dlp --yes

install-fedora-deps:
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
