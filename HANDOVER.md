# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-27 · **Status:** v2.0.0 (M6) — Mac on launchd, Pi service installed and standing by; decoder cutover is Manoj's checklist below
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
  counts, per-node honest status, telnet clients, UDP sent/failed) and
  `/api/spots`. The web page itself is still the M0 stub shell — the real
  dashboard is M5.
- **M3 update (2026-08-27):** the burn-in binary now ingests the five
  1.x cluster nodes too (config read from the app's UserDefaults into the
  local `config/dxca.toml` — gitignored). Within a minute of restart:
  VU2OY/N2WQ-2/Meridian/UberSDR-CWskim proven **Live** (Meridian = dxca's
  lifted client logged into meridian's own server), VE7CC sitting
  honest-yellow "Connected, unproven" — the exact 2026-08-24 failure mode
  the honest-status machinery exists for.
- **M4 update (2026-08-27):** the burn-in binary has users+alerts.
  `data/cty.xml` bootstrapped from the 1.x app cache (402 entities
  loaded). **Waiting on Manoj**: create the admin account
  (`POST /api/setup`), then PUT his ClubLog credentials + Telegram
  settings and `POST /api/clublog/refresh` — credentials are his to
  enter, deliberately not migrated from the 1.x UserDefaults by Claude.
- Remaining burn-in gap vs 1.x: no spots-table UI / LoTW markers (M5).
  Aggregation, cluster ingest, RUMlog feeds, and (once the account is
  set up) ClubLog classification + Telegram alerts are at parity.

## M6 progress

**2026-08-27 — v2.0.0 packaged and deployed to both hosts.**

- Version bumped to 2.0.0 (workspace + web-ui).
- `install.sh` (shack-rule compliant: auto-detect macOS/Pi + confirm +
  manual override, never silent): macOS installs a **launchd agent**
  `com.vu2cpl.dxca` (RunAtLoad + KeepAlive, log `~/Library/Logs/dxca.log`);
  Pi installs `/opt/dxca` + a **systemd service** running as `vu2cpl`
  (prebuilt binary preferred, config/data seeded only when absent —
  never clobbers a live install). Templates in `deploy/`.
- `deploy/pi-deploy.sh` — one-command cross-compile + rsync + remote
  install (the plan §9 "one binary + one TOML" deploy).
- **Mac**: the nohup burn-in was replaced by the launchd agent
  (reboot-proof at last); account/db/state untouched.
- **Pi (noderedpi4 = 192.168.1.169)**: dxca v2.0.0 active+enabled under
  systemd in /opt/dxca. State migrated from the Mac (sqlite3 .backup of
  dxca.db → same login works; cty.xml; lotw-users.txt). Its config ships
  the five cluster nodes **disabled** (no dual cluster logins while the
  Mac instance runs) and passthrough aimed at RUMlog on the Mac
  (192.168.10.226:2237 — note the Mac is on the .10 subnet, the Pi on
  .1; routed both ways, verified).

## The decoder cutover (Manoj's checklist — the last M6 box)

When ready to make the Pi the production aggregator:

1. **Decoders** (all on the Mac): change the UDP server IP from
   `127.0.0.1` to `192.168.1.169`, ports unchanged — MSHV Network Config
   (2333), JTDX Reporting primary UDP (2334), WSJT-X Reporting UDP
   (2335). The 2233 ADIF→RUMlog paths stay `127.0.0.1` — untouched.
2. **Pi web UI** `http://192.168.1.169:7580` (same login): System tab →
   tick the five nodes' **On** boxes → Apply & save.
3. **Mac**: stop the local instance so it releases the cluster logins:
   `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.vu2cpl.dxca.plist`
   (and delete the plist if permanent).
4. **RUMlogNG**: DX Cluster tab → connect to `192.168.1.169:7575`
   (Data Port 2237 needs no change — the Pi's passthrough already
   targets the Mac).
5. Verify on the Pi dashboard: sources counting, nodes Live,
   click-to-fill in RUMlog.

Rollback = reverse: decoders back to 127.0.0.1, re-bootstrap the Mac
agent (`./install.sh macos`), disable the Pi's nodes (or
`sudo systemctl stop dxca` on the Pi).

## M5 progress

**2026-08-27 (later) — M5 remainder done: web config editing with
hot-apply.**

- `PipelineState` gains swappable internals: `broadcaster()` accessor over
  a RwLock (apply_destinations swaps a fresh UdpBroadcaster — counters
  reset, 1.x `configure` behaviour) and a source-listener registry keyed
  (name, port) with **bind-first** apply: additions bind before anything
  is torn down, so a port clash rejects the whole edit; removals abort
  their tasks (socket drops, port freed).
- `NodeManager::apply` — diff by name + config fingerprint; removed or
  changed clients retire on a blocking task (a supervisor join can block
  up to its connect timeout, never on the async runtime); `start_node` is
  `&self` now (interior mutability).
- `Config` is `Serialize` (scalars declared before array-of-tables —
  TOML emitter requirement), `Config::save` rewrites `config/dxca.toml`
  with a "managed by the web UI" header; hand comments live in the
  example file.
- `GET/PUT /api/config/global` (admin): the three arrays hot-apply +
  persist; unique-name validation; `web_bind`/`telnet_port`/dedupe/ring/
  `data_dir` are returned read-only (file-edit + restart, shown in the
  UI).
- System page: full editors for sources / nodes / destinations with
  add/remove rows, format dropdown, sources-CSV allowlist, unfiltered
  flag, and one **Apply & save** button.
- `tests/config_editing.rs` proves the loop end to end over the real API:
  baseline passthrough → admin edits (source A→B, destination re-pointed,
  node added) → new port live, old port re-bindable, passthrough
  byte-identical at the new destination, old destination silent, node
  dialing, TOML reloaded with the new arrays; duplicate names 400;
  unauthenticated 401.
- Browser-verified on a disposable instance (System page renders all
  editors). Note for future sessions: the embedded browser pane's
  click/type occasionally goes stale right after navigation — re-run
  read_page and retry, or drive fetch() via javascript_tool; Manoj's own
  setup through the real UI worked first time.
- Burn-in restarted on this build. **Manoj created his account** (users:
  1) — ClubLog credentials/refresh + Telegram are his next clicks, and
  node/source editing is now in the System tab.

**2026-08-27 — M5 core complete: the real dashboard, live over a
WebSocket, verified in the browser against a disposable test instance.**

- Server: `/api/stream` WebSocket (per-session spot frames through the
  shared `annotate_spot`, status frames every 5 s), `lotw.rs` port
  (download + 1.x parse/lookup rules; global list in UserService, admin
  `/api/lotw/refresh`, `is_lotw` on every annotated spot),
  `/api/telegram/test`. axum grew the `ws` feature; a registry
  inconsistency forced `futures-util` pinned to 0.3.31 in the lockfile.
- Web UI (Svelte 5, GitHub-dark): session bootstrap → first-run **setup**
  card / **login** card / tabbed main shell. Pages: **Spots** (status
  pills incl. three-state node badges with last-spot age; filters:
  sources, bands, new-only, CQ-only, 60 s hide-duplicates like 1.x
  `displayedSpots`; sortable columns; per-user alert row tints; green
  LoTW dot), **My ClubLog** (credentials + alert levels + refresh with
  counts), **My Alerts** (Telegram + test button), **Users** (admin
  list/create), **System** (server/source/node detail, LoTW refresh).
- Verified live in the embedded browser: setup card on the burn-in;
  on a throwaway instance (scratch data dir, port 7581, since removed) —
  login, six injected spots rendered with LoTW dot on K1JT, and a
  seventh spot appearing at the top **via the WebSocket with no reload**.
- Burn-in restarted on the M5 binary; `data/lotw-users.txt` bootstrapped
  from the 1.x cache (234,467 users). Setup still pending for Manoj —
  the web setup card now replaces the curl commands.
- **M5 remainder** (deliberate scope cut, matching plan §10's "admin
  config editing hot-applies"): sources/nodes/destinations are still
  edited in `config/dxca.toml` + restart; the System page says so. Also
  future polish: spots-table columns for ΔT/low-confidence, per-user
  display-filter persistence.
- Design note: a user with **no matrix** gets no classification at all
  (no alert column, no beacon labels) — deliberate divergence from 1.x,
  which classified everything NEW DXCC against an empty matrix.

## M4 progress

**2026-08-27 — M4 complete: SQLite users, session auth, per-user ClubLog
matrices over the shared stream, Telegram fan-out. Exit criterion proven
in an end-to-end test through the real flows.**

- Deps added (plan §1): rusqlite (bundled), argon2, rand, sha2,
  ureq(+json)/rustls, flate2.
- `dxca-core`: `LogMatrix::build_from_adif` — the exact 1.x
  `ClubLogClient` build loop as a production fn; the local_parity test
  now golden-tests THIS fn against the Swift app's matrix.json.
- `dxca-connect`: `clublog.rs` (cty.php + getadif.php, gzip-by-magic,
  endpoint bases overridable for tests), `telegram.rs` (sendMessage,
  HTML, base overridable). LoTW users list deferred to M5 (display
  marker).
- `dxca-server`:
  - `db.rs` — SQLite (0600): users / sessions / per-user configs
    (clublog + notify JSON) / matrix cache. Secrets at rest plaintext by
    design (plan §5), documented trade-off.
  - `auth.rs` — argon2 PHC hashes; 256-bit tokens, SHA-256-hashed in the
    sessions table, HttpOnly SameSite=Lax cookie, 30-day TTL.
  - `users.rs` — UserService: global resolver (data/cty.xml),
    per-user matrices in memory backed by DB, the 1.x refresh flow,
    per-user classification, Telegram fan-out with the 1.x per-callsign
    cooldown (clamped 5–60 min) and the exact 1.x message format.
  - `api.rs` — full route set (plan §7): /api/setup (first-run admin
    only), login/logout/me, per-user clublog+notify config, refresh,
    admin user management, and /api/spots with per-session
    classification annotations. Composition moved out of main.rs so
    tests drive the real router.
  - Pipeline broadcasts processed spots (`spot_events`); a fan-out task
    classifies per user per spot.
  - Config: `data_dir` (default `data/`), `clublog_base_override` /
    `telegram_base_override` test knobs.
- **Exit test** (`tests/users_alerts.rs`): fake ClubLog (gzipped cty +
  per-user ADIF) and fake Telegram behind real HTTP; two accounts set up
  through the API; both refresh through the real flow; one spot on the
  shared stream → A sees `worked`, B sees `newDXCC`; exactly one
  Telegram ping, to B's bot token; the cooldown suppresses the repeat;
  anonymous /api/spots carries no classification; second /api/setup is
  refused; admin-created accounts don't hijack the admin's session.
- 1.x divergence (deliberate): `maybeNotify` also gated on the display
  filters; server-side notifications gate on levels + cooldown only
  until M5 settles per-user display filters.

## M3 progress

**2026-08-27 — M3 complete: cluster-node ingest with the honest-status
graft, validated against fake nodes in tests and the five real shack
nodes live.**

- `dxca-connect/src/dxcluster/` — the **Meridian lift** (plan §6):
  `client.rs` (sans-I/O ClientSession + supervisor thread) and the
  client half of `wire.rs` (ParsedSpot, classify_line, dx_command),
  diff-minimal with `// DXCA:` markers on every graft:
  - password prompt support (1.x node auth);
  - **honest status**: new `ClientEvent::Proven` fires only on real
    evidence (node prompt, welcome-keyword line, spot/WWV/announce) —
    never on the 30 s login-timeout fallback, which readies the session
    but leaves the pill yellow;
  - 1.x reconnect schedule (10/30/60/120/300 s, last repeats), attempt
    resets **only on proven** — never on bare TCP;
  - watchdog in the connection loop: unproven for `auth_timeout_s`
    (120) → recycle; proven but rx-silent for `silence_timeout_s`
    (15 min) → recycle; both take the normal backoff path;
  - Telnet IAC stripping ported from the 1.x client (N2WQ AR-Cluster
    banners).
- `dxca-server`: `nodes.rs` (NodeManager — per-node status map, event
  consumer thread, `handleClusterSpot`-parity synthetic decodes: message
  `CQ <call>`, SNR/mode scraped from the comment with the 1.x mode-list
  order); pipeline generalized to `PipelineInput::{Datagram,Cluster}`
  with a shared `process_spot` tail; `[[cluster_nodes]]` config;
  per-node status in `/api/status`.
- Tests: 6 session unit tests (password flow, welcome ack,
  timeout-not-proven, no-login feeds, IAC) + the M3 exit-criterion
  integration tests: a fake node proving Live end-to-end into ring +
  telnet, and a **deliberately flaky node** (accepts TCP, never acks)
  staying unproven while the watchdog recycles it with escalating
  attempts.
- Known divergence (documented choice): meridian's spot-line parser
  requires a valid `HHMMZ` time token; the 1.x parser tolerated its
  absence. Lines without one classify as `Line` events and don't count
  as spots/proof. Revisit if a real node exhibits it.

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

1. **The decoder cutover** (checklist above) — Manoj's hands, ~5 minutes.
   After it: flip the 1.x repo HANDOVER/README to maintenance mode and
   refresh `docs/UDP-PIPELINE.md` there for the Pi-centred wiring.
2. **Manoj: ClubLog + Telegram** (if not already done) — My ClubLog
   (credentials + Refresh) lights the highlighting; My Alerts wires
   Telegram. Do it on whichever instance is production (the DBs diverged
   at Pi-deploy time; after cutover the Pi's is canonical).
3. Post-2.0 backlog (plan): per-user telnet feeds (Meridian server lift),
   MQTT status/LWT on `shack/dxca/status`, durable spot history, possible
   Meridian integration (plan §6).

## Conventions (see ~/.claude/CLAUDE.md)

- **CDP** — Commit, Document, Push together on every substantive change.
- Repo is **private**; goes public only on explicit instruction.
- Credit VU3ESV (concept) and Meridian (telnet engines) in any user-facing
  write-up — already in README.
