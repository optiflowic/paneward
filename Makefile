.PHONY: all build run fmt fmt-check lint test deny audit clean

# Local quality gate, the same checks as CI except `make build`.
# `deny` and `audit` need the tools pinned in mise.toml (`mise install`).
all: fmt-check lint test deny audit

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

# Online audits (e.g. pinned commits vs. tags) run when a GitHub token is set in
# GH_TOKEN, GITHUB_TOKEN or ZIZMOR_GITHUB_TOKEN:
#   GH_TOKEN=$(gh auth token) make audit
audit:
	zizmor --persona=pedantic .

clean:
	cargo clean
