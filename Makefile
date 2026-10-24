.PHONY: all build run fmt fmt-check lint test deny clean

# Same checks as CI
all: fmt-check lint test deny

build:
	cargo build --release --locked

run:
	cargo run

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

lint:
	cargo clippy --all-targets --locked -- -D warnings

test:
	cargo test --locked

deny:
	cargo deny check

clean:
	cargo clean
