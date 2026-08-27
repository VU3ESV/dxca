# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-27 · **Status:** v2.0.0 IN PRODUCTION on noderedpi4 — cutover complete, all milestones closed
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

Lineage: original concept by Vinod VU3ESV; DX-cluster telnet client
lifted from `~/projects/meridian` (`crates/meridian-core/src/dxcluster/`),
and the web GUI's design system from the same repo's
`web-ui/default/src/` (app.css + the theme module and switcher).
**Production runs on noderedpi4 (192.168.1.169) since the 2026-08-27
cutover**; the 1.x macOS app is the retained fallback (maintenance mode).

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

## Burn-in log (Mac phase, 2026-08-27 — superseded by the Pi cutover above)

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

## Cutover — COMPLETE (2026-08-27)

Manoj executed the checklist the same evening: decoders repointed at
192.168.1.169 (all three counting on the Pi), the five nodes enabled via
the System tab (four proven Live immediately, VE7CC honest-yellow as
usual), RUMlog connected to the Pi's telnet server, passthrough to the
Mac's RUMlog clean (0 failures). The Mac launchd agent is stopped; its
plist remains in ~/Library/LaunchAgents (rollback = `./install.sh macos`
or just `launchctl bootstrap`). **Production DXCA = the Pi.** The Mac
databases are now historical; the Pi's /opt/dxca/data/dxca.db is
canonical.

## The decoder cutover (original checklist, kept for rollback reference)

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
- Web UI (Svelte 5; GitHub-dark at M5, restyled to Meridian's design
  system on 2026-08-27 — see "Web UI look" below): session bootstrap → first-run **setup**
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

Nothing operational — **v2.0.0 is fully live**: ClubLog and Telegram are
configured and working on the Pi (confirmed by Manoj 2026-08-27), so
per-user highlighting and alerts run in production.

2026-08-27 (late): the deploy tooling was **generalized for third-party
installs** — dxca.service is a template (`__USER__` → the invoking user),
install.sh chowns to the invoker, and a fresh install self-bootstraps
(setup card, cty/LoTW download on demand). Validated by re-running the
installer on the production Pi (identical result, service undisturbed).
Remaining before any public release: x86-64-Linux (+ optional Windows)
release artifacts, a Windows build test, then the repo-public flip +
vu2cpl.com card with the VU3ESV credit line.

## Alert levels 2.1 (2026-08-27)

**Four levels became eight, and the band/mode narrowing arrived on both
tabs.** The old `alert_unconfirmed` was a *global switch* that swapped the
whole comparison to the confirmed sets — so "never worked" and "worked but
unconfirmed" were mutually exclusive and the UI could never say which kind
of gap a spot was.

The ladder now runs, rarest first:

| | never worked | worked, not confirmed |
|---|---|---|
| entity | `newDXCC` | `unconfDXCC` |
| band | `newBand` | `unconfBand` |
| mode | `newMode` | `unconfMode` |
| slot | `newSlot` | `unconfSlot` |

`raw_level` decides the whole `New*` half against the **worked** sets first;
only a spot whose slot really is in the log reaches the **confirmed** sets.
That ordering is the meaning — a band never worked beats a band worked and
unconfirmed. `unconfDXCC` is checked before the narrower `?` levels because
with nothing confirmed for an entity the band/mode/slot gaps are all true.

`alert_unconfirmed` was **retired, not migrated**: serde ignores the leftover
key and the only stored account had it `false`. All four `?` levels default
off, so an existing account behaves exactly as it did until something is
ticked.

**Three narrowings, three scopes** — worth keeping straight, since "alerts"
now appears on three tabs:

- **My ClubLog** — which levels this account flags *at all* (the classifier's
  `alert_*`). Off here means the level never reaches the feed or Telegram.
- **Spots** — which flagged levels are on screen. Client-side, kept in
  `localStorage['dxca.spotfilter']`, deliberately NOT server-side: it is a
  per-browser view preference and persisting it would make it a second
  account setting to reconcile with My Alerts.
- **My Alerts** — which levels ping Telegram (`notify_*`), plus
  `notify_bands` / `notify_modes`. **Empty list = ALL**, the same convention
  `broadcast_destinations.sources` uses — which is why a fresh account is not
  silent.

New endpoints: `GET /api/reference` (bands, mode classes, the level ladder
with labels — served so the UI cannot drift from `AlertLevel`) and
`GET /api/me/station` (callsign + `MatrixStats` + QSO count for the Spots
station card). Confirmed-DXCC there follows the **award** rule: an entity
with at least one confirmed slot, not an entity fully confirmed.

Level colour is a CSS data table — `[data-level=…]` in app.css resolves
`--lvl` / `--lvl-bg`, and the feed row, Alert cell, chips and config lists
all read those two. A ninth level needs no CSS. The `?` half reuses its New
counterpart's hue pulled toward `--muted`, so hue says *which axis* and
saturation says *how badly you need it*.

`bands::SELECTABLE_BANDS` (160M–70CM) is narrower than `BANDS` on purpose:
LF/MF and microwave are still *resolved* from frequency, they just aren't
worth a checkbox here.

## DXSpider bells ate every spot (fixed 2026-08-27)

Adding **db0sue.de:8000** (DO5SSB-2, DXSpider 1.57) looked like a connection
failure. It wasn't: the node connected, logged in, and proved **Live** — and
then delivered nothing.

Its spot lines end `… 0508Z\x07\x07\r\n` — DXSpider rings the terminal on
every spot. BEL is not whitespace, so `str::trim` left it stuck to the last
token; `parse_spot_line`'s rightmost-`HHMMZ` search wants a 5-char token and
saw a 7-char `0508Z\x07\x07`, found no time, returned `None`, and every spot
fell through to the raw `Line` arm. **A node that reads healthy while
dropping 100% of its traffic** — worth remembering as a failure shape.

`wire::strip_c0_controls` now runs where lines are cut in `on_bytes` (tab
kept — it is real field whitespace), so the parse, the telnet fan-out and
the broadcaster all see the clean line. Two tests pin it, captured off the
wire; the wire-level one asserts the *un-stripped* line still fails to
parse, so the guard can't rot into a tautology.

DB0SUE is now the **fifth node** in `/opt/dxca/config/dxca.toml` (Live,
delivering). Config backup before the edit:
`/opt/dxca/config/dxca.toml.bak-before-db0sue`.

Note for triage: `/api/spots` fills `band` / `dxcc_name` / `alert` /
`is_beacon` **only for an authenticated session** (`annotate_spot`, api.rs)
— an unauthenticated `curl` shows them null and that is by design, not a
classification bug.

## Local toolchain wart (2026-08-27)

`/usr/local/bin/cargo` + `/usr/local/bin/rustc` (a standalone Rust install)
**shadow the rustup shims** on this Mac, and that install ships no
`cargo-fmt`, `cargo-clippy`, or `rustdoc`. So `just gate`'s lint step and
`cargo test`'s doctests both die with "no such command" / "could not execute
rustdoc" — nothing to do with the code. Run the gate through the toolchain's
own bin dir until it's sorted:

```sh
TC=~/.rustup/toolchains/stable-aarch64-apple-darwin
PATH="$TC/bin:$PATH" "$TC/bin/cargo" fmt --all --check
PATH="$TC/bin:$PATH" "$TC/bin/cargo" clippy --workspace --all-targets -- -D warnings
```

Real fix when there's time: remove the standalone install so rustup's shims
win (`/usr/local/bin/{cargo,rustc,...}`), or put `~/.cargo/bin` ahead of
`/usr/local/bin` on PATH.

## Deploying to a Pi that is NOT this shack's (2026-08-27)

`deploy/pi-deploy.sh --no-seed <user@ip>`. Always, for any host that isn't
noderedpi4.

Default (seeded) mode ships `config/dxca.toml` and `data/{cty.xml,
lotw-users.txt,dxca.db}` alongside the binary, installed by install.sh
*only when absent*. On a box that already has its own files that guard makes
it a no-op — which is why it is safe for our own redeploys and dangerous
everywhere else, because a **fresh** host has nothing to guard against:

- `data/dxca.db` holds ClubLog app passwords, API keys and the Telegram bot
  token **in plain text** (by design, README §Secrets) plus account password
  hashes. Seeding it onto someone else's Pi hands all of that over.
- `config/dxca.toml` holds the cluster nodes with `login_call = "VU2CPL"`.
  Two hosts on the same node with the same callsign make DXSpider kick the
  duplicate, so both ends flap.

`--no-seed` ships only the binary, `deploy/dxca.service` and `install.sh`
(not even the vu2cpl-named macOS plist). The remote box self-bootstraps: the
first-run setup card creates *their* admin account, and cty.xml / the LoTW
list download on demand. Either way the script now prints a **manifest** of
what is about to leave the machine before it rsyncs — read it.

Remote-host preflight, because the binary is cross-compiled for aarch64 +
glibc ≥ 2.36:

```sh
ssh user@ip 'uname -m; ldd --version | head -1; . /etc/os-release && echo $PRETTY_NAME; sudo -n true && echo SUDO_NOPASSWD || echo SUDO_NEEDS_PASSWORD'
```

Wants `aarch64`, glibc 2.36+, Bookworm. 32-bit Pi OS or Bullseye needs a
different target triple. Over a VPN use the **IP** — `.local` mDNS names
generally don't resolve across the tunnel. The final install step now runs
under `ssh -t` so a host without NOPASSWD sudo can actually prompt.

## Deploy gotcha (fixed 2026-08-27)

`install.sh` ended with `systemctl enable --now dxca`. `--now` starts an
*inactive* unit and does nothing to an active one, so re-running the
installer over the live service **installed a new binary but kept the old
process running** — `sudo install` replaces the file, and the running
process holds the old inode. That is why the 2026-08-27 (late) installer
re-run read as "service undisturbed": it was, including the part that
should have been disturbed. Caught when deploying the UI restyle: the
process had started 03:17 while `/opt/dxca/dxca` was stamped 03:43.

Now `enable` + an unconditional `restart`. If you ever see the dashboard
not matching the code you just shipped, check
`systemctl show dxca -p MainPID,ActiveEnterTimestamp` against
`ls -l /opt/dxca/dxca` first.

## Web UI look

**2026-08-27 — the GUI was restyled to Meridian's design system.** Visual
only: same screens, same data, same information architecture, no API
change. Rust untouched, so only `just web` is the relevant gate (it
passes; `dist/` is gitignored and rebuilt by `just web` / `just run`).

What replaced what:

- `web-ui/src/app.css` is now a port of Meridian's stylesheet. The old
  base was hardcoded GitHub-dark (`--bg: #0d1117` …) — one appearance,
  with hexes re-typed per component. The new one derives every surface
  from the CSS **system colours** `Canvas` / `CanvasText` via
  `color-mix()`, so light and dark both come for free.
- New `web-ui/src/lib/theme.svelte.ts` + `ThemeSwitcher.svelte` (both
  ported): Auto / Light / Dark in the header, stored under
  `localStorage['dxca.theme']`, applied by pinning `color-scheme` on
  `<html>`. `index.html` re-reads that same key **before** first paint to
  avoid a flash — change one, change both.
- Shared vocabulary in app.css, used by every screen instead of per-view
  hexes: `.card`, `.pill`, `.status-dot` (`.on` / `.warn` / `.err` —
  replaces the old `.dot.green/.yellow/.red`), `.filter-chip`,
  `.settings-form`, `.hint`, `.actions`, `nav.tabs`, `.popup-menu`.
- **DXCA's own addition, with no Meridian counterpart:** the *alert
  ladder* (`--alert-dxcc` / `-slot` / `-band` / `-mode`, each with a
  matching `-bg` row wash). Same four hues 1.x used, re-expressed with
  `light-dark()` — the old `rgba()` tints only composited correctly over
  `#0d1117`. Level colour and row wash come from the one token, so the
  Alert cell and its tint cannot disagree.
- Header gained the version beside the wordmark (read off the bootstrap
  `/api/status` call — no extra request) and the theme toggle.

Licensing: the three ported files are **Apache-2.0**, like
`dxca-connect/src/dxcluster/`, not MIT. Each carries the note in its own
header; README's License section lists them. If you add to app.css,
mark it `DXCA:` at the site.

Verified in the embedded browser across all six screens (Spots, My
ClubLog, My Alerts, Users, System, and the setup/login card) in **both**
appearances, against a throwaway static server with stubbed `/api`
responses — deliberately not against the production Pi, since running a
second server locally would dial the real cluster nodes with the
production callsign.

Not done (out of the visual-only scope, would need new endpoints or
panels): the propagation / host / band-activity cards Meridian's own
dashboard carries, and any i18n.

Post-2.0 backlog (pick up whenever): per-user telnet feeds (Meridian
server lift), MQTT status/LWT on `shack/dxca/status` (broker is
localhost on the Pi!), durable spot history + search, possible Meridian
integration (plan §6), web editing for bind-level scalars, additional
decoder ports if the shack grows.

## Conventions (see ~/.claude/CLAUDE.md)

- **CDP** — Commit, Document, Push together on every substantive change.
- Repo is **private**; goes public only on explicit instruction.
- Credit VU3ESV (concept) and Meridian (telnet engine + the web GUI's
  design system) in any user-facing write-up — already in README.
