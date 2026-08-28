# Interactive telnet — cluster command passthrough

**Status:** design, not built · **Drafted:** 2026-08-28 · **Phase:** 2 (post-2.0)

Today DXCA's telnet server is a one-way loudspeaker: it shouts spots at
whoever connects and ignores everything they say. This document designs the
upgrade to a real interactive session — a logger or a plain `telnet` client
connects, issues cluster commands (`sh/dx`, `sh/wwv`, `dx`, filters), and the
commands reach the upstream DX-cluster nodes DXCA is already connected to,
with the replies routed back to the operator who asked.

The short version of the recommendation: **the login gate ships first, or at
the same time — never after.** Everything else here is plumbing; that one is
a genuine change in exposure. See [Safety](#safety-read-this-before-building).

---

## 1. What exists today

More than you would expect. The outbound half is built and simply unused.

| Piece | State | Where |
|---|---|---|
| Send a command line to a node | **Built** | `ClusterClient::send_line` ([client.rs:378](../crates/dxca-connect/src/dxcluster/client.rs)) |
| Submit a spot as a `dx` command | **Built** | `submit_spot` + `wire::dx_command` ([wire.rs:195](../crates/dxca-connect/src/dxcluster/wire.rs)) |
| Gate on "session actually logged in" | **Built** | session `send_line` returns `false` unless `ready()` ([client.rs:329](../crates/dxca-connect/src/dxcluster/client.rs)) |
| Node handles kept live and addressable by name | **Built** | `NodeManager.clients: HashMap<String, (fingerprint, ClusterClient)>` |
| Node replies parsed and classified | **Built** | `ClientEvent::{Line, Announce, Wwv}` |
| Node replies *delivered anywhere* | **Missing** | dropped in an empty match arm ([nodes.rs:167](../crates/dxca-server/src/nodes.rs)) |
| Telnet client input read | **Missing** | discarded ([telnet.rs:87](../crates/dxca-connect/src/telnet.rs)) |
| Telnet login | **Missing** | no auth of any kind; binds `0.0.0.0:7575` |

So the work is: stop throwing replies away, start reading input, add a login,
and solve the one genuinely hard problem — deciding whose reply is whose.

## 2. Goals and non-goals

**Goals.** An authenticated telnet session can select one of the configured
upstream nodes and issue commands to it, receiving that node's responses
inline in the spot stream. Unauthenticated sessions keep working exactly as
today (banner, spots, input ignored) so no existing logger breaks.

**Non-goals.** DXCA does not become a cluster node: no user-to-user talk, no
`announce` to other DXCA clients, no store-and-forward, no node peering. It
also does not attempt to *emulate* DXSpider's command set — commands are
passed through to a real node, which is the authority on what they mean.

## 3. The three decisions

### 3.1 Authentication

Reuse the account system rather than inventing a second one. The pieces are
in place: `db.user_by_callsign()` returns the user and stored hash, and
`auth::verify_password()` is argon2.

```
Login: VU2CPL
Password: ********
Welcome VU2CPL — 5 nodes, current: DB0SUE. Type HELP.
```

A session that sends a blank callsign, or fails, drops to the read-only feed
it gets today. That preserves every existing logger without a config change,
which matters — RUMlog, Logger32 and N1MM+ are all pointed at 7575 already
and none of them knows how to log in.

**Honest limitation to document, not paper over:** telnet is plaintext. The
password crosses the LAN in the clear and appears in any packet capture.
That is acceptable for a shack LAN service and unacceptable over the open
internet, which is the same posture the web UI already takes (`0.0.0.0` bind,
documented in the README's exposure note). If this ever needs to leave the
LAN it needs TLS or an SSH tunnel, and the docs should say so plainly rather
than implying the login makes it safe.

### 3.2 Which node a command targets

Five nodes are configured; a command needs exactly one destination. Fanning
out to all of them is wrong — `sh/dx K1JT` would return five interleaved
result sets with no way to tell them apart.

**Decision: each session carries a "current node", defaulting to the first
proven-live one.** A `SET/NODE <name>` command switches it, `SH/NODES` lists
what is available with live status. This mirrors how an operator thinks
("I'm on DB0SUE right now") and keeps every command unambiguous.

### 3.3 Response correlation — the hard part

The cluster protocol has no request IDs. A reply to `sh/dx` arrives as a
burst of ordinary lines, indistinguishable from the node's ambient chatter,
and with several sessions connected there is nothing in the bytes that says
whose reply it is.

Three approaches were considered:

| Approach | Correlation | Cost |
|---|---|---|
| Broadcast a node's non-spot lines to every session attached to it | None — everyone sees everyone's replies | Leaks one operator's queries to another |
| Time window: route lines to the last commander for N seconds | Heuristic | Breaks the moment two sessions overlap |
| **Per-node command queue, one outstanding at a time** | **Exact** | Adds latency under contention |

**Decision: the queue.** Each node gets a serialized command slot: at most
one command in flight, its output routed to the session that issued it until
a terminator or timeout, then the next command starts. Cluster commands are
issued at human pace, so serialization costs essentially nothing in practice,
and it is the only option that does not leak one operator's results to
another. Under contention a session sees `Queued behind 1 command…` rather
than silence — an honest wait beats a mysterious one.

**Terminating a response** is unavoidably heuristic, since nodes differ.
Close the window on the first of: the node's prompt line reappearing; a
quiet period (~2 s) with no further output; or a hard timeout (~15 s). On
timeout the session is told the reply may be incomplete rather than being
left to guess — the honest-status principle the node client already follows.

**Spots are never captured by the window.** `ClientEvent::Spot` continues
straight into the pipeline as it does today; only `Line`, `Announce` and
`Wwv` are candidates for routing. A `sh/dx` reply *looks* like spots, which
is the one place this rule bites: those lines parse as spots and will flow
into the spot pipeline as though freshly spotted. See
[Safety](#safety-read-this-before-building).

## 4. Command handling

Sessions send lines; DXCA classifies each before it reaches a node:

- **Local commands**, answered by DXCA without touching a node:
  `HELP`, `SH/NODES`, `SET/NODE <name>`, `BYE`/`QUIT`, and `SH/DXCA` for
  local status (sources, node states, spot counts) — the telnet mirror of
  the web dashboard's status bar.
- **Passthrough commands**, forwarded to the current node, subject to the
  allowlist below.
- **Refused commands**, answered with a one-line explanation naming the
  reason. Never silently dropped.

### Allowlist, by role

The `users.role` column already distinguishes `admin` from `user`.

| Class | Examples | Who |
|---|---|---|
| Read-only queries | `SH/DX`, `SH/WWV`, `SH/WCY`, `SH/HEADING`, `SH/QRZ` | any logged-in account |
| Session filters | `SET/FILTER`, `ACC/SPOT`, `REJ/SPOT` | any logged-in account |
| **Spotting** | `DX <freq> <call> <comment>` | **admin only, off by default** |
| **Node account mutation** | `SET/*` that persists (name, QTH, home node), `UNSET/*` | **refused outright** |

The last row is the one that matters. Those commands change the *shack's*
account on someone else's node, persistently, and no DXCA user should be
able to reach them through a passthrough. Refuse by default with a message
pointing at the real telnet client — an operator who genuinely needs to
reconfigure a node account can connect to it directly.

Because node command sets vary, the allowlist matches on the leading verb
with prefix rules (`SH/`, `SHOW/`, `ACC/`, `REJ/`) rather than an exhaustive
table, and unknown verbs are refused rather than forwarded. Fail closed.

## 5. Safety — read this before building

**Spotting under your callsign is the real exposure.** Every node connection
logs in as `login_call` from `config/dxca.toml` — VU2CPL on this shack. A
passthrough `DX` command therefore spots *as the station owner*, no matter
who typed it. Port 7575 currently binds `0.0.0.0` with no authentication at
all; that is harmless only because the server cannot currently be talked to.
Adding passthrough without the login gate would mean anyone on the LAN can
transmit spots under the shack callsign and issue commands against its node
accounts. Hence: **login first, spotting admin-only and off by default.**

**This ships to other people's Pis.** The third-party install at
`adersh@192.168.1.151` takes whatever the next release contains, and its
node logins are *his* callsign. A default-open command port would expose his
station too, from a release he did not ask for. Default the whole feature to
**disabled** in config, so an upgrade never silently opens a port's
capabilities.

**Spot loop-back.** A spot sent upstream comes back down from that node (and
often from the four others), gets synthesized and rebroadcast. The 60-second
dedupe window absorbs the common case, but a spot echoing back after the
window closes would be re-announced as though new. Mark locally-submitted
spots and suppress the echo explicitly rather than relying on dedupe timing.

**`SH/DX` output looks exactly like live spots.** Those lines are historical,
sometimes hours old, and letting them into the pipeline would inject stale
spots into everyone's feed and Telegram alerts. While a command window is
open on a node, `DX de` lines from that node must be routed to the
requesting session **only** — not parsed into the pipeline. This is the
subtlest bug in the whole feature and the one most likely to be discovered
in production by someone getting an alert for a QSO from last Tuesday.

## 6. Shape of the code

- **`dxca-connect/src/telnet.rs`** grows a real session: line buffering, a
  login handshake, per-session state (account, current node, queue position).
  The Meridian server half (Apache-2.0, see the `dxcluster/mod.rs` licence
  note) can be lifted as planned — but note it brings its own auth model, so
  reconciling it with the SQLite accounts is part of the work, not a freebie.
  **Keep the licence boundary explicit:** lifted code stays Apache-2.0 and
  marked, exactly as the client half is.
- **`dxca-server/src/nodes.rs`** stops discarding `Line`/`Announce`/`Wwv`
  and instead offers them to a router; adds `send_line(node, line)` over the
  existing `clients` map.
- **New `dxca-server/src/cmdrouter.rs`** owns the per-node queues, the
  in-flight window, terminator detection and timeouts. This is where the
  design's only real complexity lives, and it is testable without sockets:
  feed it events, assert routing.
- **`config`** gains `[telnet] interactive = false`, `allow_spotting = false`,
  and the response timeouts — all defaulting to today's behaviour.

## 7. Milestones

1. **Plumbing, no auth.** Route node replies to a single hard-coded session;
   prove the queue and terminator logic with tests. Nothing user-facing.
2. **Login gate.** Callsign+password against the accounts table; unauthenticated
   sessions keep the read-only feed. *No passthrough yet.*
3. **Read-only passthrough.** `SH/*` and friends, with the allowlist and the
   `SH/DX`-must-not-reach-the-pipeline rule. Feature flag defaults off.
4. **Spotting.** Admin-only, opt-in, with loop suppression. Separate milestone
   because it is the only step that transmits.

Stopping after 3 would be a perfectly good place to stop.

## 8. Open questions

- **Should an unauthenticated session still get spots?** Recommended yes, for
  logger compatibility — but it does mean the port stays readable by anyone
  on the LAN, exactly as today.
- **One current node, or a broadcast query mode?** `SH/DX` against all five
  and merging results is genuinely useful for DX hunting; it also multiplies
  the correlation problem. Deferred, not rejected.
- **Does RUMlog send anything on connect?** It currently gets away with
  whatever it sends because input is discarded. Before enabling a login
  prompt, capture what each logger actually transmits — a logger that opens
  with a stray line could find itself failing a login it never meant to
  attempt. **This should be checked before milestone 2, with a packet
  capture, not assumed.**

## 9. Testing

The queue, terminator detection and allowlist are pure logic and belong in
unit tests with synthesized `ClientEvent` streams — no sockets, no node.
Integration coverage follows the pattern `users_alerts.rs` already uses: a
fake node on a local port, a real telnet session against an ephemeral port,
asserting that a command reaches the fake node, its reply reaches the right
session, and a *second* session sees none of it. That last assertion is the
one that proves the correlation design, and it should exist before the
feature is enabled anywhere.
