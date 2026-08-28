# DXCA 2.0 — Rust + web GUI port plan

**Status:** COMPLETE — all milestones M0–M6 closed 2026-08-26/27;
**v2.1.0 in production on noderedpi4** (v2.0.0 shipped) since the 2026-08-27
cutover. `HANDOVER.md` carries the per-milestone record and the post-2.0
backlog. Drafted 2026-08-26 in the 1.x repo (`docs/DXCA2-RUST-PLAN.md`
there); this copy is canonical.
**Target:** this repo — a headless Rust server with a Svelte web GUI,
running 24/7 on a Raspberry Pi (and equally on macOS/Windows/Linux),
replacing the SwiftUI app the same way SkimServer Mac became Meridian.
**New requirement vs. the Mac app:** multi-user — a login on the web GUI so
each user carries their own ClubLog credentials, log matrix, alert
preferences, and Telegram target, over one shared spot stream.

Lineage: original concept and reference implementation by Vinod VU3ESV
(FT8 Cluster Aggregator); DXCA 1.x is the macOS rewrite; DXCA 2.0 is the
cross-platform successor. The VU3ESV credit line carries into the new
repo's README. The DX-cluster telnet engines are lifted from Meridian
(same author, `thomasbasil/meridian`).

---

## 1. Shape of the new repo

Mirrors Meridian's layout at one-third scale — small Cargo workspace, one
web UI directory, a Justfile wrapping plain cargo/pnpm commands.

```
dxca/
├── Cargo.toml                 # workspace, pinned deps (Meridian style)
├── rust-toolchain.toml        # stable + rustfmt + clippy
├── Justfile                   # build / test / lint / web / dist
├── crates/
│   ├── dxca-core/             # pure logic, no I/O (≈ meridian-proto/core role)
│   │   ├── spot.rs            #   Spot model + dedupe key
│   │   ├── wsjtx.rs           #   WSJT-X binary UDP codec (NEW — see §3)
│   │   ├── cty.rs             #   CTY.DAT/cty.xml parser  (port of CTYParser)
│   │   ├── adif.rs            #   ADIF parser             (port of ADIFParser)
│   │   ├── dxcc.rs            #   DXCC resolver
│   │   ├── matrix.rs          #   per-user worked matrix  (port of LogMatrix)
│   │   ├── classify.rs        #   New + ? DXCC/Slot/Band/Mode (port of AlertClassifier)
│   │   ├── beacons.rs         #   NCDXF/IBP + national DB (port of BeaconDatabase)
│   │   ├── bands.rs, modes.rs #   BandResolver, ModeNormalizer (DATA grouping)
│   │   └── format.rs          #   DX-cluster line formatter (port of ClusterFormatter)
│   ├── dxca-connect/          # I/O engines
│   │   ├── dxcluster/         #   LIFTED from meridian-core/src/dxcluster/
│   │   │                      #   client.rs 638 + server.rs 893 + wire.rs 1064
│   │   │                      #   + mod.rs 268 — keep diff-minimal (§6)
│   │   ├── wsjtx_udp.rs       #   UDP listener on 2237 (NEW, tokio UdpSocket)
│   │   ├── broadcast.rs       #   UDP destinations incl. passthrough (port, §3)
│   │   ├── clublog.rs         #   ClubLog API client (ureq + flate2 gzip)
│   │   ├── lotw.rs            #   LoTW user-activity CSV download
│   │   └── telegram.rs        #   Bot API sendMessage (ureq)
│   └── dxca-server/           # composition root
│       ├── main.rs            #   tokio runtime, config load, wiring
│       ├── config.rs          #   global config (TOML, §4)
│       ├── db.rs              #   SQLite (rusqlite bundled): users, sessions,
│       │                      #   per-user config, matrix cache, spot store
│       ├── auth.rs            #   argon2 hashes, session cookies (§5)
│       ├── api.rs             #   axum REST routes (§7)
│       ├── stream.rs          #   WebSocket live spot/status stream
│       └── assets.rs          #   embedded web-ui/dist (include_dir) → one binary
└── web-ui/                    # Svelte 5 + Vite + TS + pnpm (crib meridian web-ui/default)
    └── src/…                  #   pages in §8
```

Dependency budget (Meridian discipline — few, pinned once): `tokio`,
`axum`, `serde`/`serde_json`, `toml`, `rusqlite` (bundled), `socket2`,
`thiserror`, `flume` + new for DXCA: `ureq` (rustls — ClubLog/LoTW/Telegram
are HTTPS; Meridian's `http1.rs` is plain-HTTP only), `flate2` (ClubLog
gzip), `argon2`, `rand` (session tokens), `include_dir`.

License: **MIT**, matching DXCA 1.x — CORRECTED post-release: Meridian
is a joint project (Basil Thomas W6BT, Vinod VU3ESV, Ram VU3RDD), not
solely Manoj's, so the lifted `dxcluster` module stays **Apache-2.0**
with attribution (LICENSE-APACHE at the root) rather than being
relicensed. The plan's original "same copyright holder" assumption was
wrong.

## 2. What is lifted vs. ported vs. new

| Piece | Source | Work |
|---|---|---|
| Cluster telnet **client** (reconnect/backoff, login) | meridian-core `dxcluster/client.rs` (638) | Lift; graft DXCA v1.8.x "honest status" semantics (§3) |
| Cluster telnet **server** (callsign login, command surface) | meridian-core `dxcluster/server.rs` (893) + `wire.rs` (1064) | Lift near-verbatim |
| WSJT-X binary UDP codec | Swift `WSJTXMessageParser/Builder` (381) | Port — highest-care item (§3) |
| UDP broadcaster + passthrough | Swift `UDPBroadcaster` (316) | Port (already raw sockets; v1.8.3 fail-counter semantics) |
| ClubLog / LoTW / Telegram clients | Swift (563) | Port onto ureq |
| Parsers + classifiers + beacon DB (`dxca-core`) | Swift Protocol/Utils/Models (~1,300) | Port; pure logic, golden-tested against Swift outputs |
| Web UI | — (SwiftUI ContentView 1,613 dies here) | New, cribbing Meridian Svelte patterns |
| Auth / multi-user | — (nothing exists anywhere) | New (§5) |
| Resources (beacon list, bundled cty) | `DXClusterAggregator/Resources` | Copy across |

## 3. High-care technical items

**WSJT-X codec.** QDataStream framing: big-endian, magic `0xadbccbda`,
schema ≥ 2; strings are u32-length UTF-8 QByteArrays; `0xFFFFFFFF` = null.
Port `WSJTXMessageParser.swift` + `WSJTXMessageBuilder.swift` field-for-field.
Validation: capture real datagrams from WSJT-X *and* JTDX (they differ in
minor fields) into `tests/vectors/*.bin`, and golden-test the Rust codec
against the Swift implementation's parse of the same bytes before wiring
anything downstream.

**Passthrough broadcast.** DXCA v1.8.2/1.8.3 semantics exactly: relay every
raw incoming decoder datagram verbatim to passthrough destinations (keeps
RUMlog click-to-fill), fire-and-forget UDP, and no phantom fail counting
for passthrough destinations. `docs/UDP-PIPELINE.md` in the 1.x repo is the
normative description of the shack pipeline and must keep working unchanged
— same default ports (UDP listen **2237**, telnet **7575**) so loggers and
decoders point at the Pi with only an IP change.

**Honest connection status.** DXCA 1.8.x behaviour is the spec: per-node
badge green only when *proven live* (login acknowledged or spots flowing),
yellow while TCP-up-unproven, spot count + last-spot age, watchdog recycling
sessions that never log in or go silent 15 min. First integration task on the
lifted Meridian `ClusterClient`: audit its health model against this and
graft what's missing. Budget real time here — this took DXCA several
releases to get right; the tests from that era are the checklist.

## 4. Configuration split

Two homes, deliberately:

- **`config/dxca.toml`** (global, admin-owned, file on disk like Meridian):
  server callsign, telnet port, web port + bind address, UDP sources,
  DX-cluster nodes, broadcast destinations, dedupe window, auto-clear/
  retention, spot-log path.
- **SQLite `dxca.db`** (per-user + runtime): users, sessions, per-user
  ClubLog config + notification config (JSON blobs, mirroring the Swift
  `ClubLogConfig`/`NotificationConfig` fields), per-user matrix cache,
  ClubLog/LoTW download cache metadata, optional durable spot store.

Everything hot-applies without restart (Meridian discipline; the Mac app
already behaves this way).

## 5. Multi-user model — the new requirement

**Accounts.** `users` table: id, callsign (login name), display name,
argon2 password hash, role (`admin` | `user`), created. First-run
bootstrap: if zero users exist, the web UI shows a one-time setup page to
create the admin account (no default password).

**Sessions.** Random 256-bit token in an HttpOnly cookie, sessions table in
SQLite with expiry. No JWT, no external identity — LAN service. Bind web to
LAN interface by default; if ever internet-exposed, that's a reverse-proxy/
Tailscale problem, not DXCA's.

**Ownership split.**
- *Admin:* global config (§4), user management, node/source control.
- *Each user:* own ClubLog email + app password + API key, refresh
  schedule, alert toggles, own Telegram chat ID + cooldown, own display
  filters.
  > **Built 2026-08-27:** the "refresh schedule" is `refresh_hours` per
  > account (0 = manual, default daily), with the shared LoTW list on a
  > server-wide `lotw_refresh_days` (default weekly). See HANDOVER's
  > "Automatic ClubLog / LoTW refresh".
  >
  > **Amended 2026-08-27:** "unconfirmed" is no longer one switch. The
  > ladder is eight independent levels — New DXCC/Slot/Band/Mode plus
  > ? DXCC/Slot/Band/Mode — and the narrowing by band and mode class
  > applies separately to the display and to Telegram. See HANDOVER's
  > "Alert levels 2.1".

**Per-user classification.** One shared spot pipeline; at classify time the
spot is scored against **every** user's matrix (hash lookups — trivial for
shack-scale user counts). The WebSocket stream carries per-user highlight
flags for the logged-in session; Telegram fan-out fires per user with
per-user cooldowns.

**Telnet personalization (phase 2).** Meridian's lifted server already
requires a callsign at login. Map that callsign → user account: known
callsign gets their filters (e.g. new-only) applied to their feed; unknown
callsigns get the plain global feed. Ships after web parity, not before.

**Secrets at rest.** ClubLog app passwords and Telegram tokens live in
SQLite on the Pi, file mode 0600, service user only. Documented plainly in
the README rather than pretending encryption-at-rest on the same host adds
security.

## 6. Meridian integration seam (later, optional)

Rules that keep the door open without building anything now:

1. `dxca-connect/dxcluster/` stays **diff-minimal** against meridian-core;
   every intentional divergence gets a `// DXCA:` comment. Improvements
   (e.g. honest-status grafts) are candidates to flow back upstream.
2. `dxca_core::Spot` keeps a documented mapping to `meridian_proto::Spot`
   (a `From` impl lives in one file, `spot.rs`).
3. `dxca-core` and `dxca-connect` never import axum/DB/auth — the server
   crate owns all of that. Integration later = mount those two crates
   behind Meridian's gate as a "WSJT-X UDP connector + per-user alert
   service"; the web login/user model would merge into whatever auth
   Meridian grows (it has none today).

## 7. API sketch

```
POST /api/login  /api/logout            # session cookie
GET  /api/setup  POST /api/setup        # first-run admin bootstrap only
GET  /api/spots?since&bands&sources…    # recent spots, per-user flags
WS   /api/stream                        # live spots + node/source status
GET/PUT /api/config/global              # admin
GET/PUT /api/config/me/clublog          # per-user
GET/PUT /api/config/me/notifications
POST /api/clublog/refresh               # per-user log re-download
GET  /api/status                        # nodes, sources, destinations, uptime
GET/POST /api/users                     # admin: list, create
PATCH/DELETE /api/users/{id}            # admin: edit (any subset), remove
GET  /                                  # embedded Svelte UI
```

## 8. Web UI pages (Svelte 5)

> **Superseded 2026-08-27:** this section originally specified a
> GitHub-dark theme "like the shack dashboard". The GUI now wears
> Meridian's design system instead — built on CSS system colours, so it
> carries both a light and a dark appearance. The page list below still
> holds. See HANDOVER's "Web UI look".


1. **Login** (and first-run setup).
2. **Dashboard** — live spots table (sortable columns matching the Mac
   app), filter bar (sources, bands, new-only, hide-dupes), status pills
   per node/source/destination with spot count + last-spot age (the v1.8.1
   clickable-pill idiom), per-user new-one row highlighting.
3. **Sources & Nodes** (admin) — UDP sources, cluster nodes, broadcast
   destinations incl. passthrough toggle.
4. **My ClubLog** — credentials, refresh, matrix summary (QSO count, last
   refresh), alert toggles.
5. **My alerts** — Telegram setup + test button, cooldown, levels.
6. **Users** (admin) — add/remove, reset password, role. Removal goes down
   to zero accounts (which re-arms the setup page); the last *admin* cannot
   be removed or demoted while other accounts remain.

## 9. Deployment (Pi-first)

- **Build:** native on the Pi works (Meridian precedent); faster path is
  cross-compiling `aarch64-unknown-linux-gnu` from the Mac (`cross` or
  cargo-zigbuild — decide in M0 by trying both). Web assets embedded via
  `include_dir`, so the deliverable is **one binary + one TOML**.
- **Run:** systemd unit (`dxca.service`, service user `dxca`, restart
  always), install script per the shack rule — `install.sh` branching
  macOS vs. Pi, auto-detect + confirm, no silent failures.
- **Footprint:** well under Meridian's; tens of MB RSS expected.
- Optional later: MQTT status/LWT on `shack/dxca/status` per shack
  conventions (broker 192.168.1.169, `svc` role account).

## 10. Milestones

- **M0 — scaffold.** Repo (private), workspace, toolchain, Justfile, CI-less
  local gate (fmt/clippy -D warnings/test, per Meridian), cross-compile
  spike, empty axum server serving a stub page on the Pi. *Exit: `just
  dist` produces a binary that runs on the Pi.*
- **M1 — core logic.** `dxca-core` complete: WSJT-X codec with captured
  test vectors + golden tests vs. Swift parse; CTY/ADIF/matrix/classifier
  ports with fixture tests. *Exit: codec round-trips real WSJT-X and JTDX
  captures byte-identically.*
- **M2 — spot path.** WSJT-X UDP listener → dedupe → in-memory ring →
  lifted telnet server serving spots; UDP broadcaster incl. passthrough.
  *Exit: RUMlog on the Mac clicks-to-fill from spots served by the Pi.*
- **M3 — cluster ingest.** Lifted ClusterClient wired to real nodes;
  honest-status audit/graft; watchdog parity. *Exit: status pills behave
  exactly like DXCA 1.8.3 against a deliberately flaky node.*
- **M4 — users + alerts.** SQLite, auth, per-user ClubLog download +
  matrix + classification, Telegram fan-out with per-user cooldown.
  *Exit: two accounts with different logs see different highlights on the
  same spot stream; each gets only their own Telegram pings.*
- **M5 — web parity.** All §8 pages; WS live updates; admin config editing
  hot-applies. *Exit: the Mac app's daily-driver workflow is fully
  replaceable in the browser.*
- **M6 — burn-in + release.** Dual-run validation: same sources feeding
  DXCA 1.8.3 on the Mac and DXCA 2.0 on the Pi simultaneously; diff the
  telnet output streams and alert decisions for a week. systemd + install
  docs + user manual refresh. *Exit: v2.0.0 tagged; DXCA 1.x (this repo)
  enters maintenance mode with a pointer to the successor.*

Phase-2 backlog (post-2.0): per-user telnet feed filtering (§5), MQTT
status, durable SQLite spot history + search UI, Meridian integration (§6).

**Interactive telnet with cluster-command passthrough — DONE in v2.3.0/
v2.3.1** (milestones 1–3 of
[`TELNET-INTERACTIVE.md`](TELNET-INTERACTIVE.md)): per-node command router,
opt-in login gate, read-only passthrough behind `telnet_interactive`
(default off). Milestone 4 (spotting) is specified and deliberately not
built — it is the only step that transmits.

## 11. Open decisions (settle in M0)

1. Repo name: `dxca` vs. `DXClusterAggregator` (cross-platform repo,
   macOS repo keeps its `-macOS` suffix). Leaning `dxca`.
2. Cross-compile tool: `cross` vs. cargo-zigbuild vs. build-on-Pi.
3. Web/telnet default bind: LAN interface vs. 0.0.0.0 (leaning
   0.0.0.0 with LAN assumption documented — matches every other shack
   service).
4. Whether the macOS 1.x app gains a "point at a remote DXCA" mode or is
   simply retired once 2.0 is stable (leaning: retire; the web UI runs
   fine in Safari).
