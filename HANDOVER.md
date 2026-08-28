# DXCA — Project Handover
*For continuation in a new Claude session*

**Created:** 2026-08-26 · **Last updated:** 2026-08-28 · **Status:** **v2.7.0 on noderedpi4, v2.5.0 on adersh** — spotter attribution (Source = the feed that carried it, Spotter = the station that heard it), a spots search box over call/spotter, both carried into Telegram alerts and the My Alerts history, and the **first schema migration** this database has had. Verified live: 63 of 73 spots carry a spotter (W3LPL relaying EA3EDU, DB0SUE relaying IU7DLD), local MSHV decodes correctly carry none, and the migration kept all 91 existing alert rows. `telnet_interactive = true` from v2.3.x still live. **`adersh@192.168.1.151` is also on v2.5.0** (deployed the same evening, `--no-seed`, backed up first): all 102 of his alert rows survived the migration, his account, cty and LoTW data intact, four nodes back Live, and 30 of 30 spots carrying a spotter. `telnet_interactive` stays **false** there — the feature has never been enabled on his box. **GitHub releases for v2.3.0, v2.3.1 and v2.4.0 are all unpublished** — tags pushed, no release pages, no Windows bundles.
**Repo:** https://github.com/vu2cpl/dxca (**public** — verified via
`gh repo view` 2026-08-27; the doc said "private" until then, and the
"Open items" release checklist still lists the public flip as pending)

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

## Session 2026-08-27 (afternoon) — the "2.1 wave"

Read this first: it is the index to everything that changed after the
cutover. Each item has its own section further down with the reasoning; the
M0–M6 progress logs below are history, not current state.

**Features**

| what | where |
|---|---|
| Web GUI restyled to Meridian's design system, light + dark | "Web UI look" |
| Eight alert levels (New ×4 + `?` ×4), band/mode narrowing on display and Telegram independently | "Alert levels 2.1" |
| Station card: DXCC / Challenge / Slots, worked vs confirmed | "DXCC Challenge points" |
| Automatic ClubLog (per-user, daily) + LoTW (server-wide, weekly) re-download | "Automatic ClubLog / LoTW refresh" |
| ClubLog API key moved from per-user to a server setting; cty.xml now admin-only and auto-refreshed | "The ClubLog API key is a SERVER setting" |

**Bugs found and fixed** — all three had been live and silent:

| bug | symptom | section |
|---|---|---|
| DXSpider `\x07` bells defeated the spot parser | db0sue.de proved **Live** and dropped 100% of its spots | "DXSpider bells ate every spot" |
| `systemctl enable --now` never restarts an *active* unit | production ran a **binary older than the one installed** | "Deploy gotcha" |
| `$HOST…` — non-ASCII byte after a variable | `HOST?: unbound variable` under bash 3.2 / C locale | "Shell gotcha" |

**Verified in production, not just built**

- Challenge total **2397 confirmed = exactly what ClubLog reports** for
  VU2CPL (56,815 QSOs, 320/319 DXCC, 4339/4075 slots, 2435 Challenge
  worked). That one match also validates `is_confirmed` and the band table.
- LoTW auto-refresh **fired on its first tick, 13:07**: attempt stamp
  13:07:31, success 13:07:35, file rewritten, 234,734 users live in
  `/api/status`. Next due 2026-09-03.
- Node roster on the Pi is now **VU2OY, N2WQ-2, UberSDR CWskim, Meridian,
  DB0SUE** — five Live. VE7CC was removed by Manoj (deliberate); DB0SUE
  added while chasing the bell bug.

**First third-party install (2026-08-27)** — `adersh@192.168.1.151`, a
remote Pi over VPN, Debian 13 (trixie), deployed with
`deploy/pi-deploy.sh --no-seed`. It self-bootstrapped: its own admin
account via the setup card, its own cty/LoTW downloads. **Nothing of this
station's went to it** — see "Deploying to a Pi that is NOT this shack's",
which is now the rule for any host that is not noderedpi4.

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
  `just run` sequences this correctly, and so does `install.sh` (which is
  why it always rebuilds in a source tree — see "install.sh did not install
  the web GUI" below).
- Justfile recipe comments must be a single line — `just --list` shows only
  the last comment line above a recipe.
- **The workspace needs rustc ≥ 1.88** (a `Cargo.lock` floor, not ours) and
  distro packages are below it. Declared as `rust-version` in
  `[workspace.package]` and re-checked by `install.sh` (`MIN_RUSTC`) — the
  two move together. See "The rustc floor is 1.88" below.
- **The VPN to Adersh's LAN shadows the shack LAN.** Both networks are
  `192.168.1.0/24`, and while the tunnel (`utun7`) is up macOS routes the
  whole subnet through it — so `192.168.1.151` works but the shack's own
  hosts (noderedpi4 at `.169`, the broker, gpsntp) become unreachable from
  the Mac, and `noderedpi4.local` ssh fails "Network is unreachable".
  Observed 2026-08-28 mid-deploy: the route flipped between two commands.
  There is no per-host workaround from userland worth keeping — disconnect
  the VPN to talk to the shack, reconnect for Adersh's Pi. Order multi-host
  work accordingly (shack first or Adersh first, not interleaved).

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
   (2335). *(The step used to add "the 2233 ADIF→RUMlog paths stay
   `127.0.0.1` — untouched". Those paths turned out to be unnecessary
   altogether — passthrough carries logged QSOs to RUMlog on 2237. Tick
   MSHV's **Enable Logged QSO** and configure nothing else; see the 1.x
   `docs/UDP-PIPELINE.md` § "Logged QSOs need no second feed".)*
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

*Field confirmation of the v2.4.0 spotter work (2026-08-28): Adersh's
alert history grew from 102 rows at v2.4.0 to 109 by the v2.5.0 deploy, and
**all 7 new rows carry a spotter** while the 102 older ones are correctly
empty. The migration's back-fill boundary is exactly where it should be, and
the recording path works in production, not only in tests.*

## Release convention (2026-08-28, standing)

**A tag is not a release.** Every tagged version gets a published GitHub
release with the Windows zip attached — `deploy/win-bundle.sh`, then
`gh release create <tag> target/win-bundle/dxca-<version>-windows-x64.zip`.
Manoj's instruction, after v2.3.0/v2.3.1/v2.4.0 sat as bare tags: Windows
users have no other route in, since building there needs the MSVC toolchain
the cross-build exists to avoid. Notes should cover everything since the
last *published* release, because tags can outrun releases.

## Open items → next session

**SHIPPED as v2.7.0 (2026-08-28): Telegram manual-only, and the DXCC
toggle inverted.** Live on noderedpi4 (9 nodes),
[released](https://github.com/vu2cpl/dxca/releases/tag/v2.7.0) with the
Windows zip. **adersh is on v2.5.0, two releases behind** — needs the VPN.
Verified after the upgrade that the stored notify row has no
`notify_manual_only` key at all, which is exactly right: it deserializes to
off, so Telegram behaves as it did.

- **Telegram manual-only.** `NotifyUserConfig::notify_manual_only`, applied
  in `fan_out` through `passes_skimmer()` — the same predicate idiom as
  `passes_band_mode`, so it is unit-testable rather than inline. Lives in
  the notify JSON blob, so **no migration**: an old row deserializes to
  `false` and behaves exactly as before, which has its own test. The point
  of keeping it independent of the Spots screen's Manual-only is the same
  as for band/mode narrowing — watch everything on screen, be interrupted
  only by people.
- **The DXCC toggle is inverted**, at Manoj's request: current-only is now
  the **default** (it is what the ARRL publishes and what an operator
  compares against) and the tickbox reads *include deleted entities*. The
  shared preference key changed from `currentOnly` to `includeDeleted`; a
  stale stored value simply reads as `false`, which is the wanted default.
- **Placement**: on the Spots station card the tickbox moved to the far
  right of the row, after the numbers — wedged between the callsign and the
  first total it broke the label/number/caption rhythm and read as a stray
  control. **My ClubLog's placement was left alone** — Manoj said it was
  fine there, so only its label and default changed.

Earlier the same evening, shipped as **v2.6.0**: skimmer identification and
the Spots "Manual only" filter.

Asked for as "how do I skip skimmers?" The answer through the telnet
passthrough is *you can't, deliberately*: `accept/rbn` / `reject/rbn` are
node-side filters, and DXCA shares one session per node with every account
and with the spot pipeline, so setting one would narrow everyone's feed and
persist on the node account. That refusal is correct — but it left the need
unmet, and the data was already being thrown away.

`ParsedSpot::spotter_is_skimmer` existed and was used only to decide
`is_cq`, then discarded — the same bug class as the spotter itself.
`Spot::is_skimmer` now carries it. **The marker matters because the parser
strips the `-#` to keep callsigns readable**, so without the flag `W3LPL`
(the operator) and `W3LPL-#` (his skimmer) are identical on screen. The
Spots table shows a `#` after the spotter and a **Manual only** tickbox
hides them.

Verified in a browser against a fake node emitting the *same callsign both
ways*: 6 spots → 3 with the box ticked, W3LPL's hand-typed spot surviving
while W3LPL's skimmer spot was filtered.

**Not done, and the obvious next step:** the same narrowing for Telegram.
`NotifyUserConfig` already has band/mode narrowing; a `notify_manual_only`
flag would slot in beside it, so alerts can be human-spots-only without
touching the Spots screen.


**SHIPPED as v2.5.0 (2026-08-28): "current entities only" for award
totals, and a Telegram format change.** **Live on both Pis** — noderedpi4 (9 nodes) and
`adersh@192.168.1.151` (4 nodes, account and reference data intact).
[Released](https://github.com/vu2cpl/dxca/releases/tag/v2.5.0) with the
Windows zip, per the new standing rule above. No schema change since
v2.4.0, so upgrading is a binary swap.

**Current-entities toggle.** DXCC has 62 deleted entities in cty.xml (Abu
Ail, Aldabra, Blenheim Reef, British North Borneo…). cty.xml has always
carried `<deleted>`, and the parser has always read it — but only to decide
whether to build a prefix rule, after which it was discarded. `DxccEntity`
now keeps the flag, `DxccResolver::deleted_adifs()` exposes the set, and
`LogMatrix` gained `stats_excluding` / `by_band_and_mode_excluding`. The
matrix itself stays resolver-free — it stores what was *worked*, not what
currently *scores* — so the caller, which holds both, supplies the set.

`/api/me/station` sends **both** sets (`stats` + `stats_current`,
`by_band_mode` + `by_band_mode_current`), so the tickbox is instant; the
payload is a dozen integers and a round trip per toggle would cost more.
The preference is shared between the Spots station card and the My ClubLog
statistics via `web-ui/src/lib/awards.svelte.ts`, deliberately — two cards
disagreeing about which entities count is worse than either answer alone.
`*_current` is **null when no cty.xml is loaded**, and the tickbox hides
itself: showing unfiltered numbers under a "current only" label would be a
quiet lie.

Verified end to end against a seeded matrix (3 current + 4 deleted
entities) with the real 402-entity cty.xml: DXCC 7→3, Challenge 42→30,
confirmed 26→18, and the per-band table dropping 40M/20M/15M from 7 to 3
while 30M correctly stayed at 3 (the deleted entities were never worked
there).

**Telegram format**, at Manoj's request: `Spotted by: X via Y` became
`Spotter: X   Node: Y` on its own line, with the time below it. Labelled
rather than prose because those two labels are what you scan for on a
phone. A local decode shows only `Node:` — an empty `Spotter:` would read
as missing data rather than as "us".


*(Resolved 2026-08-28: the two world-readable database copies left in
`adersh@192.168.1.151:/tmp` during the v2.4.0 migration check were deleted
as soon as the VPN came back; his box now has nothing of mine in `/tmp`,
and the intended `dxca.db.pre-v2.4.0` backup remains at 0600. The lesson
stands and is worth keeping: **copying a database off a box and deleting
the copy belong in one command**, not two steps separated by a network that
can vanish — a `chmod 644` on a file holding ClubLog passwords and Telegram
tokens should never outlive the scp that needed it.)*


**DEPLOYED as v2.4.0 (2026-08-28): spotter attribution + spots search.**
Live on noderedpi4 and verified against the real feed, not only tests: 63 of
73 spots carried a spotter, the migration preserved all 91 alert rows, and
`dxca.db.pre-v2.4.0` sits beside the database on the Pi as a rollback.
Three of four requests from Manoj; the fourth ("local spots not showing
modes") is **unresolved and still his to reproduce** — the live API shows
MSHV spots carrying `mode:"FT8", mode_inferred:false`, so the symptom did
not match the data and he said he would check where he was seeing it.

- **`Spot::spotter`** is a new `Option<String>` on the core model. The
  parser always extracted the spotting station; `synthetic_spot` dropped it,
  so every relayed spot was attributed to the *node* that carried it. A
  HamAlert or N2WQ feed says nothing about whose receiver heard the DX,
  which was the whole complaint. `None` for locally decoded spots.
- **Telegram** now ends `Spotted by: VU2XYZ via N2WQ-2  at 1428Z`. The
  "via" clause is suppressed when spotter and node are the same, so a
  W3LPL-fed W3LPL spot does not read "W3LPL via W3LPL". Time is the spot's
  own `hhmm()` in UTC, not delivery time.
- **Spots table** gained a sortable Spotter column beside Source, and a
  search box matching either the DX call or the spotter.

Verified in a browser against a fake DXSpider node emitting varied spotters,
in both themes — not only in tests, per the invisible-prompt lesson.

**The history carries it too** (asked for straight after): `alerts_sent`
gained a `spotter` column, and with it **`db.rs` finally has a migration
step**. `CREATE TABLE IF NOT EXISTS` is a no-op on a database that already
exists, so a new column in `SCHEMA` reaches fresh installs only — every
install in the field would have kept the old shape and then failed at the
first query naming the column. `migrate()` walks `ADDED_COLUMNS`, checks
`PRAGMA table_info`, and issues `ALTER TABLE ... ADD COLUMN` for whatever is
missing. Additive only, on purpose: `ADD COLUMN` is the one change SQLite
makes without rewriting the table, and a defaulted column cannot invalidate
an existing row. Anything needing a drop, rename or retype wants a real
versioned migration instead — do not stretch this.

`opening_an_old_database_adds_the_spotter_column_without_losing_rows` builds
a database with the **pre-migration** shape by hand, opens it through
`Db::open`, and checks the column appears, the old row survives, a new row
round-trips, and a second open is a no-op. **It earned its keep immediately**
— it caught a parameter-order bug where the spotter string was being written
into the `delivered` column, which no compiler would have flagged and which
would have corrupted every alert row in production.


**KNOWN, ACCEPTED (2026-08-28): the Spots level filter is usually empty —
and it is NOT a 2.3.x regression.** Reported as "some issue in 2.3.1", so
worth recording plainly: nothing in the v2.3.0/v2.3.1 telnet work touches
this. The filter and the 500-spot backfill both date from 2026-08-27
(`4886f7e`, `02267a1`).

The behaviour is deliberate — picking "New DXCC" makes the feed a New-DXCC
feed, not "everything, DXCC highlighted", and the comment in
`Dashboard.svelte` says so. Selecting every pill still hides *unflagged*
spots, which is why it does not help. The problem is arithmetic:

- the Spots screen backfills **500 spots** (`/api/spots?limit=500`);
- with nine nodes live the feed runs at **~105 spots/min**, so 500 spots is
  **~4.8 minutes** of history;
- genuinely new spots are rare — 24 Telegram alerts in six hours, ~4/hour;
- expected flagged spots inside a 4.8-minute window ≈ **0.3**, so roughly
  three times in four the honest answer is zero.

A five-minute keyhole onto something that happens every fifteen minutes.

**The backend is not implicated** and this was checked before blaming the
UI: the matrix holds 56,836 QSOs (refreshed 2026-08-28 11:41) and
classification demonstrably works — T5FE newBand 160M at 19:50, RI1FJL
newMode at 19:47, 24 alerts in six hours.

**Manoj's call, 2026-08-28: leave it.** Working as designed, not worth
changing now. If it is ever revisited, the fix is **server-side filtering**:
the ring holds 5000 spots (~48 minutes at this rate) and the server already
classifies per user, so a `level=` parameter on `/api/spots` applied after
`annotate_spot` gives ten times the window with a *smaller* payload than
raising the backfill limit. The weaker alternatives are a bigger limit, or
having the UI retain flagged spots as they age out.


**Milestones 1–3 BUILT (2026-08-28), 4 (spotting) deliberately not:
interactive telnet with cluster-command passthrough** —
[`docs/TELNET-INTERACTIVE.md`](docs/TELNET-INTERACTIVE.md). **Live on
noderedpi4 since 2026-08-28** as v2.3.1 with `telnet_interactive = true`;
**still off on `adersh@192.168.1.151`**, which remains on 2.2.2.

**Verified against the production server, not just fakes:** an anonymous
session throwing a bare callsign, `set/name`, `sh/dx` and `BYE` at port 7575
got **zero** non-spot bytes back while three real spots flowed through it —
so RUMlog is genuinely unaffected — and RUMlog itself reconnected on its own
after the restart (`telnet_clients: 1`). `LOGIN VU2CPL` prompts for a
password, and a wrong one is refused without revealing which half was wrong.

**v2.3.1 fixed the first field bug: "it didn't ask for password".** The
protocol was fine — a real telnet client driven through a pty got the
prompt — but nothing in the banner said `LOGIN` existed, and the
newline-less `Password: ` prompt got a spot glued to it and scrolled away.
Banner now advertises the verb; the spot feed pauses for that one session
while a password is outstanding. **The lesson: every test read the socket,
where the prompt was plainly there. Nothing tested what a human watching a
scrolling terminal sees.**

**What is still unproven: a logged-in `SH/DX` against a real node.** That
needs Manoj's account password, so it was left for him. Worth doing with the
Spots screen open beside it — the one thing the fake nodes cannot show is
that a real DB0SUE history burst stays out of the live feed.

**M3** is the passthrough itself. `commands.rs` canonicalizes an abbreviated
verb against a table of ~120 DXSpider commands and allows only the read-only
tier; `telnetcmd.rs` holds per-session state (current node, reply channel)
and joins policy → router → nodes; `NodeEventFilter` gives the router first
refusal on every node event. Three things worth knowing before touching it:
**the node is sent the canonical form** (`sh/dx 5` goes out as `SHOW/DX 5`),
because judging one string and running another is a hole, not a nicety;
**interception happens before the status counters**, so a history query does
not inflate a node's spot count; and **the design's original rule about
spots was wrong** — the doc said `ClientEvent::Spot` should always reach the
pipeline, but a `SHOW/DX` reply *is* spots, hours old, so while a window is
open every event from that node belongs to the requester. That reversal is
the single most important thing in the feature, and
`sh_dx_history_reaches_the_asker_and_nothing_else` was verified by
deliberately breaking it (remove `set_event_filter` and it fails with the
leaked callsigns listed).

**M2** added the login gate: `LOGIN <callsign>` → `Password:` → argon2
against the accounts table (via `spawn_blocking`; verifying on the async
runtime would stall every other session's spots). Gated by the new
`telnet_interactive` config key, **default false** — the port is
unauthenticated and node sessions carry the shack callsign, so it never
arrives switched on. **Login is an opt-in verb, not a prompt on connect**,
which is a deliberate change from the design: the loggers on 7575 were set
up against a server that never prompted, and a 45 s capture on the Pi showed
an established RUMlog session sends nothing at all, but connect-time
behaviour can't be seen without disconnecting a live logger. An opt-in verb
makes that unknowable question irrelevant. `an_anonymous_session_is_answered_with_silence_and_spots`
is the regression guard for every existing logger and should never be
deleted. **An authenticated session still cannot do anything** — commands
are M3.

M1 shipped the
router (`cmdrouter.rs`: per-node queue, response window, quiet + hard
timers — a pure state machine taking `now_ms` and returning actions, so it
tests without sockets or a clock) plus `NodeManager::send_line()` and
`subscribe_lines()`, and the event loop now publishes node lines instead of
discarding them. 13 new tests, 119 passing workspace-wide. **Nothing is
user-facing** — no telnet session, no auth, nothing subscribes in
production. Building on it means starting at milestone 2 (the login gate).
One thing to know before touching it: `ClientEvent::Prompt` is new, because
the prompt used to be swallowed inside the client and the router had no
completion marker; it is marked `// DXCA:` in the Apache-2.0 Meridian module
like every other graft there. The router's `on_event` returns a `consumed`
flag, and **a consumed event must not flow onward** — that is what keeps
`sh/dx` output out of the spot pipeline.
Manoj wants to issue cluster commands through DXCA to the upstream nodes.
What remains after M3: only spotting (M4), which is refused by tier and
should stay that way until someone actually wants it — it is the one step
that transmits. The doc's load-bearing points, if you read nothing else: the
**login gate ships with the feature, not after** (7575 binds `0.0.0.0` with
no auth, and every node logs in as the shack callsign, so passthrough
without auth means the LAN can spot as VU2CPL); response correlation is
solved with a **per-node serialized command queue**, since the protocol has
no request IDs; and `SH/DX` output must never reach the spot pipeline or it
injects hours-old spots into everyone's feed and alerts. Four milestones,
and stopping after the third (read-only passthrough) is a fine place to
stop.

*(Resolved 2026-08-28: v2.2.2 is on both Pis. Adersh's went out in a second
pass once the VPN came back up — the subnet clash makes a two-host deploy
inherently two passes, which is the practical cost of that gotcha and worth
planning around rather than fighting.)*

*(Resolved 2026-08-28, same morning: the v2.2.1 loose ends are closed —
noderedpi4 confirmed 2.2.1 via `/api/status` after the VPN came down, and
the GitHub release is published with the Windows zip. The retry is live on
both Pis — see "Telegram sends retry once on transport errors".)*

**TODO (2026-08-28): MQTT publishes, but nothing shows on the panadapter.**
Manoj configured the `Shack` destination against `192.168.1.169:1883` as
`svc` and reports the publish counter climbing — so DXCA's half is
confirmed working end to end: connect, authenticate, publish. What has NOT
been shown to work is the **consumer** side, from the topics to a FlexRadio
/ Aether display. Deferred deliberately; not a DXCA bug as far as anything
observed so far.

**Narrowed the same evening — the broker side is RULED OUT.** A
`mosquitto_sub -t 'shack/dxca/spots/#'` as `svc` shows both topics carrying
live traffic, correctly formed. So publishing, authentication and the ACL
are all confirmed good, and the only missing piece is that **nothing
subscribes**: there is no Node-RED flow yet bridging
`shack/dxca/spots/json` to whatever Flex or Aether consume, and neither is
known to read MQTT natively. That bridge was always separate work, not part
of the DXCA feature — a point that should have been made plainer when MQTT
shipped.

That capture also validated three of the day's spot fixes on real traffic,
which is worth keeping:

- `RI1FJL` 21270.0 from **DB0SUE**, comment `QSX 21286.10 UP 16.10 LR40` —
  no mode word at all → `"mode":"SSB","mode_inferred":true`. The Region 3
  15m phone segment, honestly labelled. Before, this was blank and scored
  DATA.
- The **same station** from **N2WQ-2**, comment starting `USB …` →
  `"mode":"USB","mode_inferred":false`. The widened mode table; `USB` was
  absent from the 1.x list of ten.
- `4X6TU` **14100.0** from VU2OY, `9 dB 20 WPM` → `"mode":"CW"`, not
  inferred. That is the parser's WPM→CW read that `synthetic_spot` used to
  throw away — and 14.100 sits in the beacon window where band-plan
  inference deliberately declines, so without it that spot would have had
  no mode at all.

Topic shapes and payloads are in the "MQTT destinations" section below.

**Credential note (2026-08-28):** the `svc` broker password was pasted into
a Claude session transcript while debugging this. Rotate it in Mosquitto and
update the DXCA MQTT destination plus the other `svc` publishers
(`monitor.sh`, chrony, ubersdr — see `vu2cpl-shack/MQTT_AUTH.md`) if that
transcript is ever shared.

Nothing operational — **v2.1.0 is fully live**: ClubLog and Telegram are
configured and working on the Pi (confirmed by Manoj 2026-08-27), so
per-user highlighting and alerts run in production.

2026-08-27 (late): the deploy tooling was **generalized for third-party
installs** — dxca.service is a template (`__USER__` → the invoking user),
install.sh chowns to the invoker, and a fresh install self-bootstraps
(setup card, cty/LoTW download on demand).

> **Correction, same day:** that generalization was recorded as "validated
> by re-running the installer on the production Pi (identical result,
> service undisturbed)". The service being undisturbed was the
> `enable --now` bug, not a pass — the installer had replaced the binary
> and left the old process running. Genuinely validated now, twice: the
> restart fix on noderedpi4, and a real third-party install on
> adersh@192.168.1.151.

Remaining before any public release: x86-64-Linux release artifacts, then
the vu2cpl.com card with the VU3ESV credit line. (The repo-public flip is
DONE — `vu2cpl/dxca` is public and carries the `DXCA v2.0.0` release. The
Windows build test is DONE — see below.)

### Windows: builds, installs, runs (2026-08-28)

First Windows build and run in the project's history. **Zero source
changes were needed** — the `#[cfg(unix)]` gates on the SIGTERM handler
and the db `0600` chmod were the only Unix-specific code, and both
fall back correctly.

- **Build:** `just win` — `cargo zigbuild --release -p dxca-server
  --target x86_64-pc-windows-gnu`. A first attempt at
  `x86_64-pc-windows-msvc` failed in exactly two places, both C, neither
  ours: `ring` (`assert.h`) and `libsqlite3-sys` (`stdlib.h`), for want of
  Windows CRT headers on the Mac. zig bundles mingw-w64's, so the GNU
  target needs no Microsoft download or licence. **A native MSVC build is
  still untried.**
- **Bundle:** `just win-bundle` → `deploy/win-bundle.sh` produces
  `dxca-<version>-windows-x64.zip` (exe + installer + uninstaller +
  README-WINDOWS.txt + licence). It refuses to ship a binary carrying the
  placeholder page.
- **Verified on `manoj@192.168.1.170`** (DESKTOP-IP8PT88, Win10 22H2
  19045, AMD64): web GUI, `/api/*`, telnet banner, SQLite creation, boot
  -triggered LOCAL SYSTEM task, survives the installing session closing,
  firewall rule + LAN reach, clean uninstall, and an **update over an
  existing install that preserves `config\` and `data\`**.
- **Still unverified:** spot ingest (no decoder or cluster node has ever
  fed it), graceful shutdown (Ctrl-C-only path), long-run stability,
  Win11/Server/ARM64, MSVC.
- **The blocker to calling it *supported*** is unchanged: `data\dxca.db`
  holds ClubLog app passwords and Telegram tokens in plain text, and the
  `0600` protection is Unix-only. Windows needs DPAPI or an ACL hardening
  pass before this is more than "works".

Four Windows gotchas, each found by testing and now handled in
`deploy/windows/install-dxca.cmd` — worth reading before writing any
other Windows script here:

1. **Batch has no `\"` escape.** Shell-style escaping produced a broken
   path and the installer silently did nothing, exiting 1 with no output.
2. **`schtasks /tr` quoting breaks on paths with spaces**, and schtasks
   has no "start in". Fixed by generating a `run-dxca.cmd` wrapper and
   registering with PowerShell `Register-ScheduledTask -WorkingDirectory`.
   Without a working directory the relative `config\dxca.toml` never
   resolves and dxca silently runs on defaults.
3. **Firewall rules scoped `profile=private` are inert on a Public
   network** — present, enabled, and doing nothing while the server
   listens. Windows re-classifies on its own when an adapter
   re-identifies; `.170` flipped to Public mid-session. The installer now
   detects it and refuses to print a LAN URL that will not answer.
4. **`timeout /t` fails under non-interactive SSH** ("Input redirection is
   not supported") and does not wait at all. Use `powershell Start-Sleep`
   in anything driven remotely.

Also learned about the Meridian box while there, and **not yet fixed in
that repo**: `meridian/HANDOVER.md` claims a `meridian-webui` firewall
rule that does not exist; `MeridianServer` has a fixed `TimeTrigger`, not
a boot trigger, so it does not survive a reboot; and its task carries
`DisallowStartIfOnBatteries` + `StopIfGoingOnBatteries`.

**Open, small:**

- The local toolchain wart — `/usr/local/bin/cargo` shadows the rustup
  shims, so `just gate`'s lint step and all doctests fail for environmental
  reasons. Workaround recorded below; the fix is to remove the standalone
  Rust install or reorder PATH.
- The Spots screen's display narrowing is per-browser (`localStorage`), not
  per-account. PLAN's "own display filters" is only half done; server-side
  persistence was deliberately deferred to avoid a second setting to
  reconcile with My Alerts.
- `udp_sent` on the Pi sat at 0 for a while after a restart and the RUMlog
  destination is `192.168.10.226` while the shack LAN is `192.168.1.x`.
  It recovered (437 and climbing), so this is a "look again if
  click-to-fill misbehaves", not a known fault.

## DXCC Challenge points (2026-08-27)

On the Spots station card, beside DXCC. **A Challenge point is entity ×
band, mode-agnostic, over ten bands only** — 160/80/40/30/20/17/15/12/10/6.

Two things to keep straight, because both are easy to get wrong later:

- **60M does NOT score.** It is in `SELECTABLE_BANDS`, the resolver emits
  it, and the spots screen offers it as a filter — but a 60m QSL adds
  nothing to the Challenge total. The WARC bands (30/17/12) *do* score,
  which is the other half of the same confusion.
- **Challenge is not this crate's "slot".** A slot is band × MODE, so a
  station worked on 20M in both CW and FT8 is two slots but one Challenge
  point. The card shows both totals, side by side, for exactly that reason.

**Validated against ClubLog itself (2026-08-27):** VU2CPL's log —
56,815 QSOs, 320 DXCC worked / 319 confirmed, 4339 slots worked / 4075
confirmed — yields **2397 confirmed Challenge points, exactly what ClubLog
reports**. That single match covers a lot: the band table, the 60m
exclusion, the entity×band (not ×mode) rule, and `Record::is_confirmed` —
including its treatment of ClubLog's own `APP_CLUBLOG_QSO_QSL = Y` flag
alongside the three standard ADIF QSL fields, which was the part with no
independent reference. If the Challenge figure ever drifts from ClubLog's,
suspect `is_confirmed` first.

`bands::CHALLENGE_BANDS` / `is_challenge_band()`, summed in
`LogMatrix::stats()` into `challenge_worked` / `challenge_confirmed`. The
award counts the confirmed figure (1000 to claim, endorsements every 500);
worked is carried alongside because the gap is the QSL chase. The unit test
`challenge_counts_entity_bands_not_slots` pins the 60m exclusion and the
one-point-per-band rule together.

## The ClubLog API key is a SERVER setting (2026-08-27)

It used to sit in each user's ClubLog config. It never belonged there: the
key is only ever used for `cty.php`, which fetches **cty.xml** — one file
backing one shared `DxccResolver` that every account is classified against.
It is not, and never was, involved in downloading anyone's log; that uses
the operator's own email + app password.

Now symmetrical with the LoTW list, which had the same shape all along:

| | cty.xml | LoTW users |
|---|---|---|
| scope | server-wide | server-wide |
| credential | `Db::clublog_api_key` | none needed |
| refresh | admin only, `POST /api/cty/refresh` | admin only |
| schedule | `cty_refresh_days` (default 7) | `lotw_refresh_days` (default 7) |

**The key is in the DATABASE, not `config/dxca.toml`.** install.sh writes
the config file 0644 and `data/dxca.db` 0600 — putting a credential in the
TOML would have moved it somewhere *more* readable. Only the cadence, which
is not secret, is a file setting.

**`adopt_legacy_api_key`** lifts a pre-2.1 per-user key to the server
setting, once, at startup — so upgrading needs no manual step. It is guarded
by its own **ran-once flag**, not by "is the server key empty?". Those look
equivalent and are not: an admin who deliberately *clears* the key leaves it
empty, and an emptiness check would re-adopt the stale key from the user's
row on the next restart, silently undoing them forever. Test:
`legacy_per_user_api_key_is_adopted_once`.

A server with no key simply keeps the cty.xml it has; the scheduler stays
quiet rather than logging a failure every 15 minutes.

**Open question (worth deciding before any public release):** the key is an
*application* credential, not a user one, so DXCA could ship a default. Two
caveats. Technically, an embedded key cannot be kept secret — the binary
must carry its own decryption key, and dxca passes it as a URL query
parameter, so tcpdump on the operator's own machine reveals it without
touching the binary. Treat any shipped key as public; don't build encryption
theatre. Practically, ask ClubLog (G7VJR) first: rate limits are per key, and
abuse by one installation would revoke it for all. If they decline, AD1C's
`cty.dat` needs no key but has no dated prefix windows or exact-call
exceptions, which `cty.rs` actively uses — a real downgrade.

## Automatic ClubLog / LoTW refresh (2026-08-27)

Both were manual-only until now — one button each — which on a 24/7 box
meant the log stopped moving whenever nobody pressed anything, and anything
worked since kept alerting as New DXCC. PLAN §5's "refresh schedule" line,
finally built.

- **Per-user ClubLog**: `refresh_hours` in the account's clublog config
  (0 = manual, default **24**). Set in the web UI, My ClubLog →
  Auto-refresh. Per-user because each account pulls its own log with its own
  credentials.
- **Server-wide LoTW**: `lotw_refresh_days` in `config/dxca.toml`
  (0 = off, default **7**). File-edited + restart, like the other scalars,
  and shown in System's file-only line. Server-wide because the list is one
  shared ~6 MB file.

`crates/dxca-server/src/refresh.rs`, spawned from main. Ticks every 15 min
and does **at most one job per tick** (LoTW first when both are due). Things
that are load-bearing:

- **Attempt stamps are separate from success stamps, and written before the
  outcome is known.** `matrices.last_refresh_unix` only advances on success,
  so a failing account would read as due on every tick and hammer ClubLog.
  `RETRY_AFTER_SECS` (1 h) is the floor either way, persisted so a crash
  loop can't reset it.
- **No refresh on boot.** The check is purely time-based; a restart pulls
  only what was already overdue.
- The LoTW **success stamp is written inside `UserService::refresh_lotw`**,
  not in the scheduler, so the manual button resets the automatic clock too.
- Timestamps live in a new **`meta`** table (`key`/`value`), *not* on file
  mtimes — `install -m 600` rewrites mtimes on every deploy and would reset
  the LoTW clock each time.
- The decision itself is a pure function, `is_due(now, last_ok,
  last_attempt, interval_secs)`, so it is unit-tested without SQLite or the
  network. Change scheduling behaviour there, not in the callers.

`ClubLogUserConfig::Default` is hand-written: deriving it would give a new
account 0 (manual) while serde's per-field default gives an existing stored
row 24, and those must agree.

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

## Account editing and deletion (2026-08-27)

Accounts used to be **create-only**: `/api/users` was `get(list).post(create)`
and the db layer had no update or delete at all, so fixing a typo'd callsign
or removing a test account meant stopping the service and editing SQLite by
hand — with `PRAGMA foreign_keys = ON` typed manually, because the CLI
defaults it off and would otherwise orphan the user's config and matrix.

Now `PATCH /api/users/{id}` and `DELETE /api/users/{id}`, both admin-gated,
with Edit/Delete buttons per row in the Users tab. PATCH takes any subset of
callsign / display_name / role / password; absent fields are left alone, so
the UI sends only what changed and an untouched password box is not an empty
string. Callsign is uppercased on write exactly as `create_user` does —
otherwise a lowercase rename would produce a row that `user_by_callsign`
(which uppercases its argument) could never match, i.e. an account nobody
can log into. A rename onto a taken callsign is checked *before* any write,
so the operator sees "already exists" rather than a raw UNIQUE-constraint
string.

**The guard rule, and why it is asymmetric.** Deleting the last account is
ALLOWED — the roster goes to zero, `/api/setup` re-arms, and that is the
intended way to start a server over. What is refused is removing *or
demoting* the last **admin** while other accounts remain: `/api/setup` only
opens at zero accounts, so that state leaves users nobody can administer and
no route back through the web UI at all. Demotion is refused regardless of
the account count, since unlike deletion it can never reach zero.

Deleting your own account is allowed and is not a special case — sessions
cascade with the row, so the cookie dies with it; the UI reloads into the
login (or setup) card. `tests/user_admin.rs` walks the whole thing end to
end, including the dead-cookie assertion after an admin deletes themselves.

Not done: no audit trail of who changed what, and no confirmation step
beyond the browser `confirm()`. Both were judged out of proportion to a
shack-scale roster of two or three accounts.

## v2.2.0 (2026-08-28)

**Windows.** dxca builds, installs and runs on Windows for the first time —
no source changes required. Ships as `dxca-2.2.0-windows-x64.zip`: a
self-contained `.exe`, an installer that registers a boot-triggered LOCAL
SYSTEM task and optionally opens the firewall, an uninstaller, and the
disclaimers. Full detail, the four batch/Windows traps it encodes, and the
list of what remains unverified are in the Windows section above.

Also in this release, previously carried by commits after the `v2.1.1` tag
and therefore never in a tagged build: MQTT spot publishing for panadapter
overlays, the server-wide call blacklist, alerts-sent history including
failures, ClubLog log statistics, the boxed status bar and chip-row Sources
filter, and the installer's behind-the-remote warning. The version string
now matches a tag containing all of it.

## v2.1.1 (2026-08-28)

Seven commits on top of v2.1.0, in three groups. Each has its own section
below with the reasoning; this is the index.

| what | why it exists |
|---|---|
| Spot-mode inference + no more silent DATA | an unknown mode was being scored into the operator's digital award slots |
| Account edit + delete (`PATCH`/`DELETE /api/users/{id}`) | accounts were create-only; fixing a callsign meant hand-editing SQLite |
| Installer: rustc gate, Node gate, real rebuild, self-verification | the VU2WJ install failed four different ways, each reporting success |

**Deployed to noderedpi4 2026-08-28 01:10 IST** with
`deploy/pi-deploy.sh --no-seed vu2cpl@noderedpi4.local`. `--no-seed` on our
own Pi too: the box already has its config and database, and the flag stops
this Mac's `dxca.db` being copied into `~/dxca-deploy/` for nothing. The
installer's own check confirmed the dashboard was serving before it exited.

Verified against live traffic straight after: 38 spots in the ring, **0 with
a blank mode**, 2 inferred — both from DB0SUE, both correct (3.749 MHz →
SSB, 7.020 MHz → CW). Small sample; the ring is in-memory and the restart
emptied it. Worth re-checking on a full ring:

```sh
ssh vu2cpl@noderedpi4.local 'curl -s "http://127.0.0.1:7580/api/spots?limit=2000"' \
  | python3 -c "import json,sys; s=json.load(sys.stdin)['spots']; print(sum(1 for x in s if not (x.get('mode') or '').strip()), 'blank of', len(s))"
```

## Missing mode on cluster spots — and the DATA default behind it (2026-08-28)

Reported as "mode is missing in some of the spots, noticed N2WQ", then
narrowed to **DB0SUE and N2WQ**. Both relay *human* spots, whose comment is
free text with no mode field, so nothing in the pipeline could name a mode.
Chasing it turned up four separate defects, the last of which was the one
that mattered.

**1. The parser's own answer was thrown away.** `wire.rs` parses mode from
comment *tokens* and additionally infers CW from a `WPM` token and RTTY from
`BPS`. `synthetic_spot` ignored `p.mode` entirely and re-scanned the comment
itself. A skimmer line commented `-15 dB 22 WPM` therefore arrived with no
mode even though the parser had already worked out CW.

**2. Substring matching invented modes.** The 1.x scan was
`comment.contains("CW")`, so `QSL via N1CW` scored CW, `tnx OM DO5SSB relay`
scored SSB, and `CWops number 123` scored CW. A wrong mode is worse than a
blank one: it files the spot in an award slot it does not belong to, and
nothing ever flags it again.

**3. The known list was ten modes and had no `USB`/`LSB`.** An ordinary
phone spot commented "USB" got no mode while the identical spot commented
"SSB" got one. `JS8`, `Q65`, `FST4`, `PSK63`, `OLIVIA`, `FM`, `SSTV` were
all likewise invisible.

**4. An unknown mode was silently scored as DATA.** This is the real bug.
`modes::canonical("")` returns `"DATA"`, and `classify` fed it straight into
the award ladder — so a 14.200 phone spot with no mode was credited to the
operator's **digital** slots, capable of firing a false New Slot/New Mode
alert and of masking a genuine phone need. Nothing about that was visible.

### What it does now

Mode is settled in three steps, best source first: the parser's token-based
`p.mode`; then a widened, **token-matched** comment scrape; then, only if the
spot genuinely says nothing, `bands::mode_from_mhz`.

The band plan is **IARU Region 3** by explicit choice (this shack's own).
Digital watering holes (`14.074` FT8, `7.0475` FT4, JS8, WSPR…) are checked
*before* the broad segments, because several sit inside a phone segment —
50.313 FT8 is in the middle of the 6m SSB range and would otherwise infer as
SSB. Segments deliberately leave gaps (beacon windows, 60m, everything above
2m): an uncertain frequency infers **nothing** rather than something wrong.

Every inferred mode is marked. `Spot::mode_inferred` rides through the API
and the WebSocket frame, and the Spots table underlines an inferred mode with
a dotted rule and a tooltip; a mode that could not be inferred at all shows
`—`. The operator can see which award slots rest on a guess.

`classify` no longer bottoms out at DATA. `modes::canonical_opt` returns
`None` for an unknown mode and `raw_level` then answers **only the band half
of the ladder** — New DXCC and New Band still report, because those are
mode-independent, while New Mode and New Slot are withheld rather than
invented. The web UI's `modeClass` mirrors the same rule, so an unknown-mode
spot matches no mode narrowing instead of hiding behind the DATA chip.

### Honest limitation

A spot's mode follows the **transmitting** station's band plan, not ours. A
Region 1 station working phone low in 40m can infer wrongly under a Region 3
table. That is why the segments are coarse and why inference is labelled
rather than asserted. If this proves annoying, the options are a
per-region table keyed on the spotted call's DXCC, or dropping segment
inference and keeping only the watering holes.

**Test gotcha worth remembering:** the ±500 Hz watering-hole tolerance is
compared in **integer Hz**. As MHz f64, `(14.0745 - 14.074).abs()` is
0.0005000000000000004 — a dial exactly 500 Hz up fell outside its own
tolerance. The test caught it; the float version would have shipped.

## install.sh now verifies its own work (2026-08-27)

Every failure in this script's history looked like a **successful** install:
the unit started, the URL printed, the script exited 0, and the dashboard
was a placeholder. Nothing in the installer ever looked at the result. So it
now finishes by fetching its own web page:

- **Pi/Linux**: `systemctl is-active` first — a unit that failed to start
  gives a far better error than a connection refused twenty seconds later —
  then the HTTP check, then the LAN URL.
- **macOS**: `launchctl print` on the agent, then the same HTTP check.
- Both poll for up to 20s (a fresh service needs a moment to bind; failing
  on the first refused connection would be a false alarm), then look for
  build.rs's `Web UI not built into this binary` marker. Finding it is a
  hard failure — **unless `--stub-ui` was passed**, in which case the
  placeholder is what was asked for and the check passes.
- `web_url` reads `web_bind` out of the installed `dxca.toml`, so a
  non-default port is probed correctly. A wildcard bind (`0.0.0.0`, `[::]`)
  is probed on loopback; a specific address is used as-is, since loopback
  would not be listening then. No config at all falls back to 7580.
- No curl on the box is a skip-with-a-note, not a failure.

Tested against fixture servers: real dashboard passes, placeholder fails,
placeholder + `--stub-ui` passes, nothing listening fails with the right
log command for the platform. `web_url` verified across all five bind forms.

**Follow-up, same day — the Node version check landed too**, closing the
asymmetry with the rustc gate. `node_gate` runs before `pnpm install`.

The rule is **not a plain floor**, which is the whole trap: vite 6 and
`@sveltejs/vite-plugin-svelte` 5 both declare `engines.node` as
`^18 || ^20 || >=22`, so the odd-numbered non-LTS releases **19 and 21 are
excluded even though they are newer than 18**. `>= 18` would wave them
through; `>= 20` would reject a working 18. `NODE_ENGINES` records the
string and the comment says to re-read those two `engines` fields before
touching it — they are the source of truth, not us.

`--stub-ui` skips the dashboard build rather than dying, same as for a
missing pnpm. A `node --version` that returns nothing is a note, not a
failure: pnpm is itself a Node program, so that state is near-impossible
and not worth failing an install over.

Tested against stub `node` binaries at 16 / 18 / 19 / 20 / 21 / 22 / 24 / 26
— accepted, rejected, and the `--stub-ui` skip all behave.

## My Alerts shows what was actually sent (2026-08-28)

Requested as "in alerts tab, alerts sent should be shown like spots list".
The fan-out is fire-and-forget on a background thread, so from the UI a spot
that was **flagged**, **narrowed away** by the band/mode chips, **held by the
cooldown**, or **refused by Telegram** all looked identical: nothing arrived.
That was unanswerable, and it is what this fixes.

New `alerts_sent` table (per user, `ON DELETE CASCADE`, indexed on
`user_id, time_unix DESC`) written by `fan_out` **after** the send, with its
verdict. `GET /api/me/alerts` serves the caller's own; the My Alerts tab
renders them in the Spots row vocabulary — level tint via `[data-level]`,
same columns — and re-polls every 15 s, because a history that only updated
on reload would be the same invisibility again.

**Failures are recorded, with Telegram's own error text**, and shown as a
`failed` chip whose tooltip is the reason. A "sent" log that stored only
successes would hide the single most useful row on the page — the shack
broker analogue is a bad chat id, which otherwise fails in silence forever.

> **Correction (2026-08-28): the level tint above was claimed, not
> delivered.** The rows carried `data-level` from the start, and this doc
> said they rendered "in the Spots row vocabulary — level tint via
> `[data-level]`" — but no rule in `Alerts.svelte` ever *painted* with the
> `--lvl` / `--lvl-bg` those attributes resolve. Dashboard's two painting
> rules (`tr.flagged td`, `tr.flagged .alert`) are scoped to Dashboard, as
> Svelte scopes all component CSS, so My Alerts showed uniformly grey rows.
> Fixed by adding the pair to `Alerts.svelte`, keyed on `tr[data-level]`
> rather than a `.flagged` class — every sent alert was flagged by
> definition, so there is nothing to gate on. Verified across all eight
> levels in both themes: each resolves a distinct wash and Level colour,
> `?` variants reading as muted versions of their `New` counterparts.
> Worth generalizing from: `data-level` on an element buys nothing on its
> own, and a third table wanting the tint will need these two rules again
> (or app.css would have to paint, which it deliberately does not).

Bounded at `ALERT_HISTORY_MAX = 500` **per user**, pruned on insert, so a
busy operator cannot evict another account's history. That is asserted, not
assumed: the unit test floods A past the cap and checks B's single row
survives.

Test coverage, honestly: `users_alerts.rs` proves the delivered path end to
end through the real fan-out — one row for B, `newDXCC`, `delivered: true`,
band and entity carried, **A's history empty** though both users saw the
same spot, and a 401 without a session. The **failure** path is covered at
the storage layer (`db.rs`, delivered=false with its error round-tripping)
rather than end to end, because the fake Telegram in that test always
answers 200 and the cooldown blocks a second alert for the same call.

## Telegram sends retry once on transport errors (2026-08-28)

Prompted by a field report: Adersh's screenshot of his My Alerts page with
red `failed` chips, "why failed?". The `alerts_sent.error` column on his Pi
(`adersh@192.168.1.151`, over the VPN) answered it — 8 of 41 alerts failed
overnight (~03:00–06:20 IST), all with one of two errors, both transport:

- `tls connection init failed: Resource temporarily unavailable (os error 11)`
  — the TLS handshake to `api.telegram.org` timed out mid-setup;
- `Network Error: timed out reading response` — connected, but no reply
  within the sender's 10 s limit.

Not a config problem: token and chat id are fine (those would fail as HTTP
4xx from Telegram), successes interleaved with the failures, and at test
time his Pi reached `api.telegram.org` in ~0.8 s consistently (IPv6,
~280 ms RTT). Classic night-time congestion blips on a residential uplink —
and with the old single-attempt sender, each blip was a lost alert.

**Fix:** `Telegram::send` now makes **one retry after 2 s, on transport
errors only**. An HTTP rejection (bad token, unknown chat) still returns
immediately — Telegram would only refuse it again, and retrying those would
double-send nothing while masking real misconfiguration. Both call sites
(`fan_out`, the test button) already run `send` under `spawn_blocking`, so
the pause cannot stall the pipeline. When the retry also fails, the recorded
error says so: `… (retried; first attempt: …)` — the My Alerts tooltip then
shows both verdicts. `retry_delay` is a public field (default 2 s) zeroed in
tests.

Tests (`telegram.rs`): a local TCP stub whose per-request closure either
answers or drops the connection. `transport_error_is_retried_once` — first
connection dropped, second answered 200, exactly 2 hits.
`http_rejection_is_not_retried` — always 400, exactly 1 hit. Stub gotcha
worth keeping: the stub must read the **full request body before replying**,
else the client's body write fails and a 400 test reads as a transport error
(that false start cost one red test run).

**Deploy status: SHIPPED as v2.2.1**, tagged and deployed to both Pis the
same morning via `pi-deploy.sh` (adersh with `--no-seed`). Adersh's Pi
verified end to end: `/api/status` reports 2.2.1, service active, cluster
nodes reconnected, his account untouched. noderedpi4's installer
serving-check passed immediately; its `/api/status` read confirmed 2.2.1
once the VPN came down (the subnet-overlap gotcha — see Known gotchas —
had cut the Mac off from the shack LAN mid-verification). GitHub release
published: https://github.com/vu2cpl/dxca/releases/tag/v2.2.1.

## My ClubLog shows the log's statistics (2026-08-28)

Requested as "My ClubLog after a refresh should show all statistics for that
user". It previously reported a refresh as one sentence — *"Refreshed: 56816
QSOs, 320 DXCC entities"* — which scrolled away and told you nothing about
the log itself.

There is now a **Log statistics** card: QSO count, log callsign and refresh
age; the six award totals (DXCC / Challenge / Slots, worked beside
confirmed); and, new, **entities per band** and **entities per mode**.

`LogMatrix::by_band_and_mode` slices the same in-memory matrix `stats()`
already walks, so this costs one pass over the entity map and **no new
storage or endpoint** — it rides `/api/me/station`, the endpoint the Spots
station card uses, so the two screens can never disagree about the log.

Deliberate choices:

- **Entities, not QSOs.** A band's figure is how many DXCC entities have at
  least one contact there, which is what the award counts. Stated on the
  card, because "20M: 3" is otherwise ambiguous.
- **Empty rows are kept**, dimmed rather than hidden. A band with nothing on
  it is the most useful row on the page.
- Ordering is `SELECTABLE_BANDS` (160M first) and `modes::CLASSES`
  (CW/PHONE/DATA), not hash-map order.
- `refresh()` reloads the card, which is the actual ask — a refresh has to
  become visible as numbers.

**Gotcha that cost a round trip:** Svelte scopes component styles, so
System.svelte's `.stats` block does **not** reach ClubLog.svelte. The six
totals rendered stacked one per line until an identical block was added
locally. Same shape on purpose — the two screens report the same numbers and
should not look like different things.

Verified by seeding a synthetic matrix straight into the `matrices` table
(four entities across 80/40/20/15/10M, partially confirmed) and reading the
rendered table back: 20M 3 worked / 2 confirmed, 40M 2 / 2, 15M 2 / 0, 10M
1 / 0 — matching the seed exactly.

## System-tab editors dragged the page sideways (fixed 2026-08-28)

Reported against the new MQTT card. Two faults, one visual and one that
looked like a data problem.

**The row is eleven columns** — name, broker, port, user, password, base
topic, client id, sources CSV, plus two checkboxes and the delete — which at
the editor's input widths comes to roughly **77rem (~1230px)**. Wider than a
1280px laptop, so the whole page scrolled horizontally and the nav slid off
the left. Every editor table is now wrapped in `.editor-scroll`
(`overflow-x: auto`), so a wide row scrolls **inside its card** and the page
never does. Measured after the fix at a 1350px viewport: page
`scrollWidth == clientWidth`, table 1240px inside a 974px wrapper that
scrolls.

**"published 0, failed 0" never moved.** The counters were only fetched on
mount and after a save, so they sat at zero while spots were in fact being
published, until a reload. They are polled on the same 5 s tick as the
server status now — via a **stats-only** `loadMqttStats()`, deliberately
separate from `loadMqtt()`: the latter replaces the `mqtt` array, which is
bound to the inputs, so polling it would have wiped whatever the operator
was halfway through typing.

Gotcha for the next person doing a bulk edit here: a `perl -0pi` replacing
`      </table>` also matched the **refdata** and **nodes-live** tables,
which are not editors, and produced two stray `</div>`s. The Svelte compiler
caught both. Wrapping by opening tag is safe; closing tags at a shared
indentation are not.

## MQTT destinations (2026-08-28)

Requested as "add an MQTT send option to destinations like FlexRadio or
Aether to display on the panadapter; setup should include broker, port,
auth". Manoj chose **both payloads** (JSON and cluster line, sibling topics)
and **one configurable base topic**, default `shack/dxca/spots` per the
shack's `shack/<service>/…` convention.

**A sibling of `broadcast.rs`, not a variant of it.** A UDP destination is a
datagram to an address; an MQTT destination is a connection with
credentials, keepalive and reconnect. Folding them into one struct would
have meant a dummy IP on every MQTT row and a dummy topic on every UDP one,
so `crates/dxca-connect/src/mqtt.rs` is its own module and MQTT rows are
their own list.

**Stored in the database, not `config/dxca.toml`.** The broker password is a
secret and that file is installed 0644 while `data/dxca.db` is 0600 —
exactly the reasoning that moved the ClubLog API key. Kept as one JSON blob
under the `mqtt_destinations` meta key: a short list edited as a whole.
Consequence: the MQTT editor in the System tab has its **own** Apply & save
button, because it does not go through `/api/config/global`.

**Dependency added: `rumqttc` 0.25, `default-features = false`** — that drops
TLS, websocket and proxy, none of which the shack broker uses. Turning
`use-rustls` back on is the change to make if a broker ever needs 8883. The
1.88 rustc floor did not move.

Notes:

- `try_publish`, not `publish`, and QoS 0: a spot feed is a live stream, so
  dropping when the outbound queue is full is right and blocking the
  pipeline on a slow broker is not. Drops are counted per destination.
- The rumqttc event loop **must** be driven or nothing is ever sent — one
  named thread per destination drains `connection.iter()`. Errors there are
  the reconnect path, not a reason to stop.
- `apply_mqtt` replaces the whole publisher, so dropping it closes the old
  connections and their threads: an edited broker address really is the one
  in use.
- Band is derived in the payload rather than carried — every consumer wants
  it and only the frequency is authoritative. Off-band frequencies publish
  `"band": null` rather than guessing.
- The same `sources` allowlist and `unfiltered` flag as UDP, and the same
  dedupe verdict, so MQTT and the logger see one consistent feed.

`tests/mqtt_publish.rs` stands up a **minimal MQTT 3.1.1 broker** — CONNECT
/ CONNACK, QoS-0 PUBLISH, PINGREQ, and nothing else — points a destination
at it through the HTTP API, pushes a spot through the real pipeline, and
reads what actually arrived on the socket: username and password on the
wire, both topics, the JSON's derived band, the cluster line, the
trailing-slash guard (`spots/` must not yield `spots//json`), and that
disabling a row stops publishing. Asserting on a config round-trip and
trusting the library would have proved none of that.

## Blacklist tab (2026-08-28)

Requested as "add a tab for blacklisted calls". Three decisions, all Manoj's:

**Server-wide, admin-managed** — one list, not per-user. The first pass at
the question offered scope and effect separately and allowed an impossible
pairing (per-user + drop-at-pipeline); the ring is shared, so a pipeline drop
cannot be per-user. Re-asked as one coupled choice.

**Drops at the pipeline**, before the ring: gone from the Spots table, the
telnet cluster server, the filtered UDP destinations and Telegram, for
everyone. Not a display filter.

**Exact match on the spotted DX call**, case-insensitive, no wildcards.

Implementation notes:

- New `blacklist` table (`callsign` PK, `added_unix`). `CREATE TABLE IF NOT
  EXISTS`, so an existing database picks it up with no migration step.
- The pipeline holds its own `RwLock<HashSet<String>>` rather than querying
  SQLite per spot — this is the hot path for every decode and every cluster
  line. `apply_blacklist` swaps it, the same hot-apply shape as sources and
  destinations, so an edit lands on the next spot with no restart. `main`
  loads the stored list before the first spot can arrive.
- The check sits **after** the `source_counts` increment on purpose: that
  counter is what proves a node is alive, and a node sending only blocked
  calls is still up. The count means "received", not "shown".
- `GET/POST /api/blacklist`, `DELETE /api/blacklist/{callsign}`, all
  admin-gated. Every write refreshes the live set as well as the database.

**Known limitation, stated in the UI and the README:** the verbatim UDP
passthrough forwards decoder datagrams *before* parsing (that is what makes
click-to-fill work), so a blocked call inside a WSJT-X decode still reaches a
logger by that path. Cluster spots have no passthrough and are dropped
completely. Closing that would mean parsing before passthrough and giving up
1.x byte-verbatim parity — deliberately not done.

`tests/blacklist.rs` drives the real API and the real pipeline: baseline
through, block, next spot never reaches `/api/spots` while the pre-block one
survives, idempotent re-add, unblock, spot flows again, 404 on removing
something unlisted, 401 for anonymous.

## Status bar boxed by category; Sources became a chip row (2026-08-28)

Two reports on the Spots screen, both fixed together because they are the
same strip of UI.

**The status pills were one long horizontal line**, and every cluster node
appeared in it **twice** with identical counts — `DB0SUE 110` and
`DB0SUE 110 10s`. Not two bugs: `process_spot` increments `source_counts`
for *every* spot, so a cluster node lands in `spots_per_source` as well as
in `cluster_nodes`, and the flat row rendered both maps end to end. It read
as duplication because it was.

Now four labelled boxes — **Decoders**, **Cluster nodes**, **Feeds out**,
**Reference** — sized to their contents and wrapping, the Meridian shape.
Decoders is the sources that are *not* nodes, so a node's count appears once
in the box that also carries its state and age. An idle decoder list says
"nothing decoding" rather than vanishing, because a decoder that has stopped
feeding is exactly what you want to notice.

**Sources was a `<details>` checkbox dropdown** while every other narrowing
on the screen was a chip row, so it hid both what was available and what was
picked. It is the same `ChipGroup` as Alerts / Modes / Bands now: All, then
one chip per source, empty set meaning everything. The bespoke `toggle()`
helper, the `.menu` / `summary` / `details` CSS and the `.fsep` separator all
went with it — the build is warning-free rather than carrying dead selectors.

Verified on a real instance rather than by eye: a throwaway server pointed at
noderedpi4's own telnet feed on 7575, so real spots populated both maps and
the duplication case actually occurred.

## "CQ only" filtered nothing (fixed 2026-08-28)

Reported as "cq only has no effect". Measured on production before touching
anything: **800 of 800 spots started with "CQ "**, across all five sources.
The checkbox was wired correctly; it had nothing to bite on.

`synthetic_spot` built every cluster spot as `message: format!("CQ {}",
p.call)`, and both `Spot::is_cq()` and the UI filter tested that string. So
the answer was yes by construction. Meanwhile `wire.rs` had already worked
out the real `SpotKind` (Cq / Dx / De / Bcn / Ncdxf, `Unknown` when the
comment carries no marker) and `synthetic_spot` discarded it — the same
shape as the mode bug two sections down: real information thrown away and a
confident-looking default put in its place.

`is_cq` is now a **stored field** on `Spot`, not a derivation:

- cluster: `SpotKind::Cq | SpotKind::Dx`, **or the spotter is a skimmer**.
  Manoj chose the skimmer widening deliberately — a skimmer only reports
  stations calling CQ, so an unmarked skimmer spot is one even though its
  comment never says so, whereas an unmarked human spot is somebody logging
  a station they heard. Strict marker-only was the alternative and would
  have hidden most of the feed behind the filter.
- decoder: `message_is_cq()` on the real decoded text, where the prefix
  genuinely means something.

`message` deliberately stays `"CQ <call>"` whatever the kind, because
`dx_callsign()` parses the callsign out of it and both the outbound cluster
line (`format.rs`) and `duplicate_key` ride on that. The spotter's actual
text is carried in the new `Spot::comment` and is what the Spots table now
shows in the Message column — previously it displayed a synthesised "CQ
<call>" for every cluster spot, which was never what anyone typed.

## Users edit row overflowed the card (fixed 2026-08-28)

Reported with screenshots: editing a user at a wide window pushed the Save
/ Cancel buttons and the password field out of the USERS card and across the
ADD USER card beside it. Narrow windows were fine, which is the clue.

`.card-grid` is `columns: 26rem` — CSS multi-column, so each card sits in a
fixed ~416px track. The edit row put three `<input>`s and two buttons into
table cells, and `td { white-space: nowrap }` means those cells cannot
shrink: the table's min-content width was far wider than the track, so it
spilled into the neighbouring column. A narrow viewport collapses to a
single column as wide as the page, which is why it only showed up wide.

Setting `width: 100%` on the inputs would not have fixed it — an `<input>`
carries an intrinsic min-content width from its default `size`, and table
layout honours that. Inline inputs were never going to fit a 26rem track.

The edit form now spans all four columns in one `<td colspan="4">` and uses
the same `.settings-form` label/field grid as the Add-user card, so it
sidesteps the table's intrinsic sizing entirely and matches the existing
visual vocabulary. `.edit-row td` is the one place the table is allowed
`white-space: normal`; roster rows stay nowrap.

Verified against a real instance rather than by eye: a throwaway server on
127.0.0.1:7599 with two seeded accounts, driven in the browser at 900,
1199 and 1500 px — no overflow at any of them — plus a save round-trip
confirming the restructured form still writes (roster updated, "Updated
VU2CPL." shown).

## install.sh does not pull — but it says when you should have (2026-08-28)

Asked directly: "will it pull latest in the install script?" It will not, and
should not. `install.sh` runs no git command that changes anything; it builds
the working tree as it stands. Pulling would be deciding about someone else's
code — fighting local edits, tripping over a detached HEAD — and it cannot
work at all from a `pi-deploy.sh` bundle, which has no repo.

The hazard is the other half: re-running the installer *without* pulling
rebuilds the OLD tree and reports success, which is indistinguishable from a
working update. That is the same shape as every other bug in this file.

`git_currency_note` therefore refreshes the remote-tracking ref only —
inside `.git`; the working tree and checked-out commit are untouched — and
prints how many commits behind upstream the tree is, before the build
starts rather than after ten minutes of compiling. It is a **NOTE, never a
stop**: installing an older checkout is a legitimate thing to do (a
rollback, a branch under test), unlike a missing dashboard.

Silent when the tree is current, when there is no upstream, and when there
is no `.git`. `GIT_TERMINAL_PROMPT=0` so a repo wanting credentials fails
rather than hanging an unattended install, and an unreachable remote says
the currency is *unknown* instead of implying the tree is current. Tested
across all five shapes: current, five behind, detached HEAD, unreachable
remote, no repo.

## install.sh did not install the web GUI (fixed 2026-08-27)

Two separate holes, both ending in the same symptom: the service comes up,
the dashboard does not. What you get instead is build.rs's placeholder —
*"Web UI not built into this binary"* — which is easy to read as a broken
install rather than a missing build step.

**1. A re-run never rebuilt.** The pi/linux branch picked the first of
`./dxca` or `target/release/dxca` that existed and, if it found one,
skipped the entire `require_cargo` / `build_web` / `cargo build` block. So
the classic recovery — hit the missing-pnpm warning, install pnpm, re-run
`./install.sh` — reused the stale binary and kept serving the placeholder
**forever**. Nothing in the output said so; the install "succeeded" each
time.

The two cases were being conflated. A git clone has `crates/` and must
always rebuild (cargo is incremental, so an unchanged tree is cheap). A
`pi-deploy.sh` bundle has no `crates/` and no `web-ui/` — just a `./dxca`
cross-compiled on the Mac with the dashboard already inside. The branch now
keys on `[ -d "$REPO/crates" ]`, and a directory with neither is a `die`
instead of a confusing half-install.

**2. Missing pnpm was a warning.** `build_web` printed a NOTE and carried
on, which is right for `cargo build` (the Meridian rule: plain builds never
need Node) but wrong for an installer — the web GUI is part of what
"install" means. It is now a hard stop naming the platform's install
command, with `--stub-ui` as the explicit opt-out for a deliberately
headless install.

`--stub-ui` meant install.sh needed real argument parsing, so it now loops
over `"$@"` like pi-deploy.sh does, and `--help` prints the header comment
via awk rather than a hardcoded `sed` line range — the range had already
silently truncated the help by the time it was first tried.

Verified with stubbed toolchains: build_web hard-stops on both platforms
with the right hint, `--stub-ui` proceeds, and the branch picks rebuild /
prebuilt / die for a clone-with-stale-binary, a deploy bundle, and an empty
directory respectively.

**Follow-up — never suggest `apt install nodejs npm`.** The first version of
that hard-stop message did, and on VU2WJ's Pi it failed outright: Node there
came from **NodeSource** (22.23.2), whose `nodejs` package provides its own
npm and declares `Conflicts: npm`, so apt refused with ~30 unsatisfiable
`node-*` dependencies. The fix on that box was simply `sudo npm install -g
pnpm` — npm was already present the whole time.

`build_web` now prints the command that fits the box it is running on:
npm present → `npm install -g pnpm`; only corepack → `corepack enable
pnpm`; macOS → `brew install node pnpm`; nothing → `apt install -y nodejs`
(**without** `npm`) then npm's own pnpm. Tested against stub PATHs for all
four shapes.

## The rustc floor is 1.88, and install.sh now enforces it (2026-08-27)

A third-party install on VU2WJ's Pi died at `cargo build` with *"rustc
1.85.0 is not supported by the following packages"* — twelve `icu_*` /
`idna_adapter` crates wanting 1.88 or 1.86. None of them are ours:

```
ureq 2.12.1 -> url 2.5.8 -> idna 1.1.0 -> idna_adapter 1.2.2 -> icu_* 2.3.0
```

The floor is therefore set by the committed `Cargo.lock`, and no manifest
in the workspace declares a `rust-version`, so cargo only complained deep
in dependency resolution — minutes in, after downloading 148 crates.

Two things made this land on a *fresh* box and never here: Debian Trixie's
`apt install cargo` gives exactly **1.85.0**, and a distro rustc ignores
`rust-toolchain.toml` (`channel = "stable"`), so it never self-corrects.
This Mac has been on 1.96.1 throughout.

`install.sh`'s `require_cargo` now checks `rustc --version` against
`MIN_RUSTC=1.88` before any build, and branches the remedy on whether
rustup is present — stale toolchain (`rustup update stable`) versus distro
package (install rustup, then confirm `which rustc` is `~/.cargo/bin`).
Comparison is major.minor via awk, so `1.99.0-nightly` passes. Verified
under `/bin/bash` 3.2 against fake toolchains at 1.85.0 / 1.88.0 / 1.96.1 /
1.99.0-nightly / 2.0.0, plus no-cargo and cargo-without-rustc.

**Follow-up, same day — `rust-version = "1.88"` is now declared** in
`[workspace.package]` and inherited by all three crates via
`rust-version.workspace = true`. Cargo now refuses in seconds with its own
message, and because the workspace is on `resolver = "3"` (MSRV-aware) a
future `cargo update` will prefer dependency versions that keep the floor
where it is instead of raising it silently. Adding it left `Cargo.lock`
untouched and `cargo check --workspace --all-targets` clean.

**There are now two constants, and they must move together:**
`rust-version` in `Cargo.toml` and `MIN_RUSTC` in `install.sh`. The
installer's check was kept on purpose — it fires before the pnpm web build
and before the first sudo, and it can name the remedy (stale rustup versus a
distro package that ignores `rust-toolchain.toml`), which cargo cannot.

Note the floor is the *lockfile's*, not the edition's: edition 2024 only
needs 1.85. If a dependency bump raises the real floor, both constants move.

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

## Shell gotcha: never put a non-ASCII byte after `$VAR` (2026-08-27)

`echo "Shipping to $HOST…"` died with **`HOST?: unbound variable`** the
first time pi-deploy.sh was run from Manoj's own terminal. Not the ellipsis
being unprintable — bash 3.2 (macOS `/bin/bash`) and any non-UTF-8 locale
treat the ellipsis's high bytes as *identifier* characters, so the variable
actually looked up was `HOST\xe2\x80\xa6`. Under `set -u` that is fatal, and
the error prints the mangled name as `HOST?`.

It had run fine every previous time because those runs were bash 5 with a
UTF-8 locale, which parses it correctly — a latent bug the whole time, not a
regression.

Rule: **no `$VAR` in a runtime string may be followed by a non-ASCII byte.**
Brace it (`${HOST}`) and keep echo/say strings ASCII; prose punctuation is
fine in comments, which never execute. Runtime strings in both scripts are
now ASCII (bar a few em-dashes with no adjacent variable). Reproduce any
suspicion with:

```sh
LC_ALL=C /bin/bash deploy/pi-deploy.sh --no-seed user@host
```

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

**Keep `--no-seed` on RE-deploys too**, not just the first install. It is
tempting to drop it once the remote box has its own config and database,
since install.sh then skips both — but that guard runs *after* the
transfer. `rsync` copies the whole staging directory to `~/dxca-deploy/` on
the remote host first, so without the flag this station's `dxca.db` ends up
sitting in someone else's home directory even though the installer
correctly declines to install it. The flag prevents the **copy**, which is
the part that matters.

What a re-deploy does and does not keep, on any host:

| | |
|---|---|
| `/opt/dxca/config/dxca.toml` | **kept** (written only if absent) |
| `/opt/dxca/data/*` — db, cty, LoTW | **kept** (same guard) |
| `/opt/dxca/dxca` | replaced — the point of the exercise |
| `/etc/systemd/system/dxca.service` | **overwritten unconditionally** from the template — any hand-editing of the unit is lost |

New schema and config keys need no manual step: the `meta` table is
`CREATE TABLE IF NOT EXISTS`, and every added key is `serde(default)`.

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
