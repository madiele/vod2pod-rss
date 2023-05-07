#!make
include .dev.env
export $$(cat .dev.env | xargs)

start-deps:
	sudo docker-compose -f docker-compose.dev_enviroment.yml up -d

test:
	cargo test -- --nocapture

test-watch:
	cargo watch "test -- --nocapture"

run:
	cargo run

hot-reload:
	cargo watch -s "redis-cli flushdb && cargo run"
