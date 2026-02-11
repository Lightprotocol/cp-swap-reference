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
    cargo clippy --workspace --features "test-sbf" --tests -- -D warnings

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

# Payer pubkey for upgradeable program deployment
payer_pubkey := "ALA2cnz41Wa2v2EYUdkYHsg7VnKsbH1j7secM5aiP8k"
accounts_dir := root_dir / "programs/cp-swap/tests/accounts"

# Start light test-validator in foreground (for manual testing)
start-validator: _stop-validator _clean-ledger
    @echo "Starting light test-validator with cp-swap program (foreground)..."
    light test-validator \
        --limit-ledger-size 50000000 \
        --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so "{{payer_pubkey}}" \
        --account-dir "{{accounts_dir}}"

# Start light test-validator in background with cp-swap program
start-validator-background: _stop-validator _clean-ledger
    @echo "Starting light test-validator with cp-swap program..."
    light test-validator \
        --limit-ledger-size 50000000 \
        --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so "{{payer_pubkey}}" \
        --account-dir "{{accounts_dir}}" &
    @echo "Waiting for validator to start..."
    @for i in $(seq 1 120); do \
        if curl -s -X POST http://localhost:8899 -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"result"'; then \
            echo "Validator is ready!"; \
            exit 0; \
        fi; \
        sleep 1; \
    done; \
    echo "Validator failed to start within 120 seconds"; \
    exit 1

# Stop light test-validator
stop-validator:
    light test-validator --stop 2>/dev/null || true

# Internal: stop any existing validator
_stop-validator:
    @echo "Stopping any existing validator..."
    @light test-validator --stop 2>/dev/null || true
    @sleep 2

# Internal: clean old ledger
_clean-ledger:
    @echo "Cleaning old ledger..."
    @rm -rf test-ledger

# Watch and rebuild on changes
watch:
    cargo watch -x "build-sbf"
