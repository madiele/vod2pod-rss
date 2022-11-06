test:
	cargo test -- --nocapture

test-watch:
	cargo watch "test -- --nocapture"

run:
	RUST_LOG=info cargo run