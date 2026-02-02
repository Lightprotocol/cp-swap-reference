# CP-Swap Reference Implementation
set dotenv-load

# Use absolute path for SBF_OUT_DIR so tests can find program binaries
root_dir := `pwd`
export SBF_OUT_DIR := root_dir / "target/deploy"

default:
    @just --list

# === Build ===

# Build the SBF program
build:
    cargo build-sbf

# Build native (for tests)
build-native:
    cargo build --package raydium-cp-swap

# === Test ===

# Run all tests (build + local + integration)
test-all: build test-local test-integration

# Run local tests only (no external validator needed)
test: build test-local

# Run local tests (LightProgramTest, no external validator)
test-local:
    cargo test --test local_test -- --nocapture

# Run functional tests (requires light test-validator)
test-functional:
    cargo test --test functional_test -- --nocapture --test-threads=1

# Run program tests (requires light test-validator with forester)
test-program:
    cargo test --test program_test -- --nocapture --test-threads=1

# Run all integration tests (starts/stops light test-validator automatically)
test-integration: start-validator-background
    -cargo test --test functional_test -- --nocapture --test-threads=1
    -cargo test --test program_test -- --nocapture --test-threads=1
    just stop-validator

# === Lint & Format ===

# Check formatting and run clippy
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-features --tests -- -D warnings

# Format code
format:
    cargo fmt --all

# === Clean ===

# Clean build artifacts
clean:
    find . -type d -name "test-ledger" -exec rm -rf {} + 2>/dev/null || true
    cargo clean

# === Info ===

# Show version info
info:
    @echo "Solana: $(solana --version)"
    @echo "Rust: $(rustc --version)"
    @echo "Anchor: $(anchor --version)"

# === Development ===

# Start light test-validator in foreground (for manual testing)
start-validator:
    light test-validator

# Start light test-validator in background with cp-swap program
start-validator-background:
    ./scripts/start-validator.sh

# Stop light test-validator
stop-validator:
    light test-validator --stop 2>/dev/null || true

# Watch and rebuild on changes
watch:
    cargo watch -x "build-sbf"
