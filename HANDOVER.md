# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-27 · **Status:** M1 complete
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

## M1 progress

**2026-08-27 — M1 complete: all core-logic ports done, full-chain parity
proven against the Swift app's own artifacts.**

- Ported (`dxca-core/src/`): `adif.rs`, `cty.rs` (with a built-in minimal
  XML scanner — no XML dependency), `dxcc.rs` (resolver + slash-portable
  normalization), `matrix.rs` (serde field names match the Swift Codable
  JSON — 1.x `matrix.json` deserializes as-is), `classify.rs`
  (AlertClassifier + AlertLevel with Swift raw-value serde names, plus an
  `AlertConfig` extracted from ClubLogConfig for the per-user model),
  `bands.rs`, `modes.rs`, `beacons.rs`. 29 unit tests codify the Swift
  behaviours, including the deliberate quirks (ADIF lengths count
  characters; header fields leak into the first record; null/invalid-UTF-8
  QStrings parse as "").
- **Parity test** (`tests/local_parity.rs`, `#[ignore]`d — needs the 1.x
  app's cache, run with `-- --ignored`): parses the real cty.xml (402
  entities, 35,817 rules) and log.adi (56,811 records), rebuilds the
  matrix the way `ClubLogClient` does, and compares against the Swift
  app's own matrix.json. **Exact match on first run**: 320 DXCC statuses
  set-for-set, 26,179 worked calls. Runs in 0.24 s (release).
- Personal log data stays out of the repo — the parity test reads
  `~/Library/Application Support/DXClusterAggregator/` locally.

**2026-08-26 — WSJT-X codec + live-captured vectors.**

- `dxca-core/src/wsjtx.rs`: full parser/builder port of the Swift
  `WSJTXMessageParser`/`WSJTXMessageBuilder`, permissive-parse semantics
  preserved exactly (required fields only for Status: clientId+dialFreq;
  Decode: all but is_new/lowConfidence/offAir; null/invalid-UTF-8 strings
  → ""; unknown type fails the parse). Builder synthesizes Status+Decode
  pairs (deCall `DXCAGGR`, schema 2); `encode_spot` takes `time_ms` from
  the caller — core has no clock.
- `tests/vectors/`: real datagrams captured off the live shack pipeline
  (tcpdump on lo0, 2026-08-26, ~12 min, all three decoders on air):
  8 samples per (decoder, type) for MSHV/JTDX/WSJT-X Heartbeat/Status/
  Decode (+ a type-6 Close from JTDX and WSJT-X), `summary.json` with full
  counts, gzipped source pcap under `raw/`. All schema 2.
- `tests/vectors_roundtrip.rs`: every vector parses; Decodes re-encode
  **byte-identically** (all three decoders); Statuses re-encode as a byte
  prefix modulo null-vs-empty strings. **Emitter quirk worth remembering:
  WSJT-X emits null QStrings (`FFFFFFFF`) for unset fields (dxCall,
  dxGrid…); MSHV and JTDX emit empty ones. The parser collapses both to
  ""** (Swift parity) — the test's `prefix_matches_modulo_null_strings`
  documents this.
- Capture also proved the 1.x passthrough invariant on live traffic:
  1094/1094 datagrams on :2237 byte-identical to a source datagram
  (M2's spec baseline). Extractor: `scripts/extract_vectors.py`.

## Open items → next session

1. **M2 — spot path** (plan §10): WSJT-X UDP listener → dedupe →
   in-memory ring → the telnet server lifted from
   `meridian-core/src/dxcluster/` (server.rs + wire.rs); UDP broadcaster
   incl. passthrough (spec baseline: the 1094/1094 byte-identical
   passthrough capture, and v1.8.3's no-fail-counting rule). Port the
   exact v1.8.x dedupe-window semantics into `Spot::dedupe_key` while
   wiring the pipeline (the M0 skeleton keys on raw kHz, deliberately
   tighter). Exit: RUMlog on the Mac clicks-to-fill from spots served by
   the Pi.
2. M3+ per docs/PLAN.md §10.

## Conventions (see ~/.claude/CLAUDE.md)

- **CDP** — Commit, Document, Push together on every substantive change.
- Repo is **private**; goes public only on explicit instruction.
- Credit VU3ESV (concept) and Meridian (telnet engines) in any user-facing
  write-up — already in README.
