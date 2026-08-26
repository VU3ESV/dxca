# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-27 · **Status:** M2 complete — dxca IS the live shack aggregator (burn-in)
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
Since 2026-08-27 dxca itself runs the shack (burn-in, see below); the 1.x
macOS app is the standing fallback until M6 signs off.

## M0 groundwork

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

## Burn-in — dxca is currently running the shack (since 2026-08-27)

**M2 exit validated live by Manoj**: with the 1.x app stopped, dxca took
over ports 2333/2334/2335 + 7575 on the Mac Mini with the default config.
RUMlog's DX Cluster tab reconnected on its own, its spots table populated
via both paths, and **click-to-fill worked** from the decoders through
dxca's passthrough. MSHV + JTDX ingesting live during validation;
passthrough 180+ datagrams, 0 failures.

Operational state:
- Runs **on the Mac** (decoders send to 127.0.0.1), detached:
  `nohup ~/projects/dxca/target/release/dxca` started from
  `~/projects/dxca` (config-relative paths), log
  `~/Library/Logs/dxca-burnin.log`. Survives Claude sessions, **not** a
  reboot — after a reboot either relaunch it the same way or fall back to
  the 1.x app.
- **The 1.x macOS app must stay closed while dxca runs** (same ports).
  Revert = `pkill -f target/release/dxca`, then launch
  DXClusterAggregator.app.
- Watch it via `http://localhost:7580/api/status` (per-source spot
  counts, telnet clients, UDP sent/failed) and `/api/spots`. The web page
  itself is still the M0 stub shell — the real dashboard is M5.
- Burn-in gap vs 1.x: **no DX-cluster node ingest yet** (that's M3), no
  ClubLog highlighting/Telegram (M4), no spots-table UI (M5). The spot
  aggregation + RUMlog feed paths are at parity.

## M2 progress

**2026-08-27 — M2 code complete: the spot path runs end to end.**

- `dxca-core`: `spot.rs` reworked into the faithful `SpotMessage` port
  (dx-callsign extraction with the full `looksLikeCallsign` heuristic,
  CALL-BAND-MODE dedupe key, decode-time→today mapping — mode stays raw,
  `"~"` and all, exactly like 1.x); `format.rs` ports `ClusterFormatter`
  (single-token spotter, pad-or-truncate cells, `HHmmZ`).
- `dxca-connect`: `wsjtx_udp.rs` (tokio source listeners), `broadcast.rs`
  (cluster/wsjtx/passthrough destinations, **v1.8.3 counter semantics** —
  passthrough skipped before bookkeeping; `unfiltered` flag honored),
  `telnet.rs` (1.x-parity server: banner, CRLF fan-out, no login —
  **deliberate deviation from the plan's "lift Meridian's server" line**:
  the 1.x server has no login so parity doesn't need it; Meridian's
  login-capable server comes with per-user telnet feeds in phase 2).
- `dxca-server`: `pipeline.rs` mirrors `ContentView.handleDecode` —
  passthrough-before-parse, per-source dial from Status, 60 s rebroadcast
  dedupe (no-callsign spots bypass dedupe and broadcast as UNKNOWN, 1.x
  parity), spot ring; config grew `[[udp_sources]]` /
  `[[broadcast_destinations]]` with shack-wiring defaults; new
  `/api/spots` + richer `/api/status`; lib target added so integration
  tests can drive the pipeline.
- **End-to-end test** (`dxca-server/tests/spot_path.rs`): real captured
  JTDX Status+Decode vectors sent over real UDP sockets → passthrough
  destination receives both byte-identical, telnet client receives the
  banner and a `DX de JTDX:` line carrying the extracted callsign, ring
  holds the spot with the Status-supplied dial. Passes.
- Display filters (bands/sources/CQ-only/new-only) deliberately absent
  from the broadcast gate until M5 decides whether per-user display
  filters should keep gating the shared feed like 1.x does.

**M2 exit box: CLOSED 2026-08-27** — the live swap-over validated it (see
the Burn-in section above).

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

1. **M3 — cluster ingest** (plan §10): lift the DX-cluster telnet
   *client* from `meridian-core/src/dxcluster/client.rs` (+wire.rs),
   graft the v1.8.x honest-status semantics (proven-live / yellow /
   watchdog), wire cluster spots into the pipeline as synthetic decodes
   the way `handleClusterSpot` does (SNR/mode scraped from the comment,
   message `"CQ <call>"`). Exit: status behaviour matches DXCA 1.8.3
   against a deliberately flaky node.
2. M4+ per docs/PLAN.md §10.

## Conventions (see ~/.claude/CLAUDE.md)

- **CDP** — Commit, Document, Push together on every substantive change.
- Repo is **private**; goes public only on explicit instruction.
- Credit VU3ESV (concept) and Meridian (telnet engines) in any user-facing
  write-up — already in README.
