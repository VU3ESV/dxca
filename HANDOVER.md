# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-26 · **Status:** M0 done
**Repo:** https://github.com/vu2cpl/dxca (private)

---

## What this is

FT8/FT4 + DX-cluster spot aggregator with a multi-user web GUI — Rust
successor to [DXClusterAggregator for
macOS](https://github.com/vu2cpl/DXClusterAggregator-macOS), Pi-first.
**The design and milestone plan is [docs/PLAN.md](docs/PLAN.md) — read it
before touching anything.** It was drafted in the 1.x repo
(`docs/DXCA2-RUST-PLAN.md` there, same content at draft time); this copy is
canonical from now on.

Lineage: original concept by Vinod VU3ESV; DX-cluster telnet engines to be
lifted from `~/projects/meridian` (`crates/meridian-core/src/dxcluster/`).
The 1.x macOS app stays the production DXCA until M6 signs off.

## Current state (M0 complete)

- Cargo workspace (edition 2024): `dxca-core` (spot model + 2 tests),
  `dxca-connect` (doc-only placeholder), `dxca-server` (bin `dxca`).
- Server stub works end to end: loads `config/dxca.toml` (defaults if
  absent, hard error if invalid), serves the embedded Svelte page and
  `GET /api/status`, graceful SIGINT/SIGTERM shutdown. Smoke-tested on
  macOS: status JSON + real Svelte dist served, clean shutdown.
- Web UI: Svelte 5 + Vite + TS under `web-ui/` (pnpm). `pnpm build` →
  `dist/` → embedded by `include_dir` at next cargo build.
  `dxca-server/build.rs` writes a stub `dist/index.html` when absent so
  plain `cargo build` never needs Node (Meridian rule).
- Local gate green: `cargo test --workspace` (4 pass incl. doc-test run),
  `cargo fmt --check`, `clippy --all-targets -D warnings`, web build.
- Pi cross-compile proven **and executed on real hardware**: `just dist` →
  1.5 MB ELF `target/aarch64-unknown-linux-gnu/release/dxca` (aarch64,
  glibc ≥ 2.36) via cargo-zigbuild. Ran on noderedpi4 (Debian 13 Trixie,
  glibc 2.41) 2026-08-26: `/api/status` JSON + embedded UI served, clean
  SIGTERM shutdown. Shack ssh is `vu2cpl@<host>`, key-auth only.

## Plan §11 decisions resolved at M0

1. Repo name: **`dxca`** (this repo, private).
2. Cross-compile: **cargo-zigbuild** (brew-installed with zig; no Docker on
   the Mac, so `cross` was out). Target pinned `aarch64-unknown-linux-gnu.2.36`.
3. Web/telnet bind: default `0.0.0.0` (LAN-service assumption, documented
   in the example config).
4. (Mac-app retirement question stays open until 2.0 is real.)

## Known gotchas

- **This Mac's Rust is Homebrew rustup** with only `cargo`/`rustc` proxies
  symlinked into `/usr/local/bin`. `cargo fmt`, `clippy`, and doc-tests
  need the full proxy set: prefix `PATH="/opt/homebrew/opt/rustup/bin:$PATH"`
  (or symlink the missing proxies like the existing two). Plain builds work
  either way.
- **pnpm blocks dependency install scripts**: `web-ui/pnpm-workspace.yaml`
  allow-lists esbuild's postinstall (same pattern as Meridian). Without it,
  `pnpm install` errors with ERR_PNPM_IGNORED_BUILDS.
- `include_dir` embeds whatever `web-ui/dist` held at **compile** time —
  after `pnpm build`, rebuild the server or you serve the old page.
  `just run` sequences this correctly.
- Justfile recipe comments must be a single line — `just --list` shows only
  the last comment line above a recipe.

## Open items → next session

1. **M1 — core logic** (plan §10): port the WSJT-X binary codec with
   captured WSJT-X *and* JTDX datagrams as test vectors (capture on the Mac
   with the 1.x app's sources live); port CTY/ADIF/matrix/classifier with
   golden tests against the Swift implementation's output. Swift sources:
   `~/projects/DXClusterAggregator/DXClusterAggregator/{Protocol,Utils,Models}/`.
2. M2+ per docs/PLAN.md §10.

## Conventions (see ~/.claude/CLAUDE.md)

- **CDP** — Commit, Document, Push together on every substantive change.
- Repo is **private**; goes public only on explicit instruction.
- Credit VU3ESV (concept) and Meridian (telnet engines) in any user-facing
  write-up — already in README.
