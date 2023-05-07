#!make
include .dev.env
export $$(cat .dev.env | xargs)

install-ubuntu-deps:
	sudo apt update
	sudo apt install ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
	pip3 install yt-dlp

install-fedora-deps:
	sudo dnf update
	sudo dnf install ffmpeg python3-pip redis
	echo installing rust + cargo
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
	pip3 install yt-dlp

start-deps:
	@if which docker-compose >/dev/null; then \
		sudo docker-compose -f docker-compose.dev_enviroment.yml up -d; \
	else \
		sudo docker compose -f docker-compose.dev_enviroment.yml up -d; \
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
