.PHONY: fmt lint test build clean check

# Format code
fmt:
	cargo fmt

# Run clippy linter
lint:
	cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
	cargo test --all-features

# Format + lint + test
check: fmt lint test

# Build release binary
build:
	cargo build --release

# Clean build artifacts
clean:
	cargo clean
