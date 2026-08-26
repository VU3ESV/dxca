# Convenience wrappers only — every recipe is a plain cargo/pnpm command
# (Meridian discipline: `just` is never required).

# Build all crates (debug).
build:
    cargo build --workspace

# Run the full local gate: fmt, clippy (warnings denied), tests, web build.
gate: lint test web

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

# Build the web UI into web-ui/dist (embedded by the next cargo build).
web:
    pnpm -C web-ui install
    pnpm -C web-ui build

# Run the server locally (builds web UI first so the real page is embedded).
run: web
    cargo run -p dxca-server

# Cross-compile a Pi release binary (ARM64 Linux, glibc ≥ 2.36 = Bookworm). Needs rustup target aarch64-unknown-linux-gnu + cargo-zigbuild.
dist: web
    cargo zigbuild --release -p dxca-server --target aarch64-unknown-linux-gnu.2.36
    @echo "Pi binary: target/aarch64-unknown-linux-gnu/release/dxca"

# Native release build for this machine.
release: web
    cargo build --release -p dxca-server
