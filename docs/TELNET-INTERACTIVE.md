# Interactive telnet — cluster command passthrough

**Status:** **milestones 1–3 built, live on noderedpi4 and proven against a
real node** — `telnet_interactive = true` there; **off** on both third-party
installs (`adersh`, `vu2wj`), which run the same binary with the feature
disabled. Milestone 4 (spotting) is designed, not built, and stopping here is
a perfectly good place to stop. The one piece of unfinished business is
`IAC WILL ECHO`, which would fix both the echoed password and the shredded
typing · **Drafted:** 2026-08-28 · **Phase:** 2 (post-2.0)

DXCA's telnet server used to be a one-way loudspeaker: it shouted spots at
whoever connected and ignored everything they said. This document designed
the upgrade to a real interactive session — a `telnet` client logs in, issues
cluster commands, and they reach the upstream nodes DXCA is already connected
to, with the replies routed back to the operator who asked — and now records
what was built, including the places the design turned out to be wrong.

The short version of the recommendation: **the login gate ships first, or at
the same time — never after.** Everything else here is plumbing; that one is
a genuine change in exposure. See [Safety](#safety-read-this-before-building).

---

## 1. What exists today

Where each piece stands after milestones 1–3.

| Piece | State | Where |
|---|---|---|
| Send a command line to a node | **Built** | `ClusterClient::send_line` ([client.rs:378](../crates/dxca-connect/src/dxcluster/client.rs)) |
| Submit a spot as a `dx` command | **Built** | `submit_spot` + `wire::dx_command` ([wire.rs:195](../crates/dxca-connect/src/dxcluster/wire.rs)) |
| Gate on "session actually logged in" | **Built** | session `send_line` returns `false` unless `ready()` ([client.rs:329](../crates/dxca-connect/src/dxcluster/client.rs)) |
| Node handles kept live and addressable by name | **Built** | `NodeManager.clients: HashMap<String, (fingerprint, ClusterClient)>` |
| Node replies parsed and classified | **Built** | `ClientEvent::{Line, Announce, Wwv}` |
| Node replies *delivered anywhere* | **Built** (M1) | `NodeManager::subscribe_lines()` |
| Node prompt as a completion marker | **Built** (M1) | `ClientEvent::Prompt` |
| Per-node command queue + response window | **Built** (M1) | `cmdrouter.rs` |
| Telnet client input read | **Built** (M2) | line-buffered, IAC-stripped |
| Telnet login | **Built** (M2) | `LOGIN`, opt-in via `telnet_interactive` |
| Command canonicalization + allowlist | **Built** (M3) | `commands.rs` |
| Session → node command routing | **Built** (M3) | `telnetcmd.rs` |
| `SHOW/DX` kept out of the spot pipeline | **Built** (M3) | `NodeEventFilter` |
| Spotting (`DX`) | **Refused** | M4; admin-only and opt-in when it lands |

What is left is milestone 4 — spotting — which is deliberately still refused.

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
LOGIN VU2CPL
Password: ********
Welcome VU2CPL (admin). Type HELP.
```

**As built this is an opt-in verb, not a prompt on connect** — see milestone
2 for why. A session that never types `LOGIN` is not prompted, not asked for
anything, and not answered: it gets the plain spot feed exactly as before.
That preserves every existing logger without a config change, which matters —
RUMlog, Logger32 and N1MM+ are all pointed at 7575 already and none of them
knows how to log in.

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

*Confirmed as a requirement by Manoj, 2026-08-28: commands go to a selected
node, explicitly never as a broadcast.*

Two details worth settling now rather than in code review:

- **`SET/NODE` is local to DXCA and must not reach a node.** It collides with
  nothing in DXSpider today, but it is DXCA's own session state; the
  canonicalizer intercepts it before the allowlist. Same for `SH/NODES`.
- **A per-command override** — `<node> <command>`, e.g. `DB0SUE SH/DX 20` —
  would let a one-off query skip switching nodes. **Not built:** it needs
  care, because a bare node name as the first token has to be told apart
  from a mistyped command, and getting that wrong turns a typo into a
  command aimed at the wrong node. `SET/NODE` covers the need for now.

**What happens when the current node is not live?** Refuse with the node's
actual state rather than queuing indefinitely — `DB0SUE is Reconnecting;
pick another with SET/NODE or wait`. Silently holding a command until a
reconnect would be the honest-status rule broken in a new place.

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

**Spots ARE captured by an open window** — reversed from this document's
first draft, which said the opposite and was wrong. The original reasoning
was that `ClientEvent::Spot` should always flow to the pipeline and only
`Line`/`Announce`/`Wwv` should be routable. But a `SHOW/DX` reply *is* a
burst of `DX de …` lines, they parse as spots, and they are hours old. Under
the original rule they would have gone straight into the live feed. So while
a window is open on a node, **every** event from that node belongs to the
requester, spots included, and the pipeline sees none of them. Ambient spots
resume the instant the window closes, which for a `SHOW/DX` is a fraction of
a second later. See [Safety](#safety-read-this-before-building).

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

### Abbreviation makes a denylist impossible

DXSpider states it plainly: *"All commands can be abbreviated, so SHOW/DX can
be abbreviated to SH/DX, ANNOUNCE can be shortened to AN and so on."* The
manual documents **no minimum length and no uniqueness rule**, so the same
command arrives in many spellings and the node resolves them itself.

This is load-bearing, and it kills the obvious implementations:

- **A denylist cannot work.** To block `SET/HOMENODE` you would have to
  enumerate `SET/HOME`, `SE/HOMENODE`, `S/HOME`, and every other prefix a
  node might accept. Miss one and it goes straight through.
- **Prefix string-matching on `SH/` cannot work either** (an earlier draft of
  this document proposed exactly that, wrongly). `SH/` does not identify a
  show command — `S/H` might also reach one, and `SET/…` shares the `S`.

**Therefore: canonicalize, then allowlist.** Keep a table of the DXSpider
commands DXCA knows, expand each incoming verb to its canonical full form
against that table, and forward only what the allowlist admits *after*
expansion. Anything that does not expand to a known command — ambiguous,
unknown, or a spelling the table cannot resolve — is refused with a message
saying so. Fail closed, every time.

The cost is that DXCA must carry a command inventory and keep it roughly
current, and that a node-specific command DXCA has never heard of gets
refused even though the node would accept it. That is the correct trade:
the alternative is forwarding unrecognized text to a session authenticated
as the station owner. An operator who needs an exotic command can telnet to
the node directly.

## 5. Safety — read this before building

**Spotting under your callsign is the real exposure.** Every node connection
logs in as `login_call` from `config/dxca.toml` — VU2CPL on this shack. A
passthrough `DX` command therefore spots *as the station owner*, no matter
who typed it. Port 7575 currently binds `0.0.0.0` with no authentication at
all; that is harmless only because the server cannot currently be talked to.
Adding passthrough without the login gate would mean anyone on the LAN can
transmit spots under the shack callsign and issue commands against its node
accounts. Hence: **login first, spotting admin-only and off by default.**

**`DX` can spot on someone else's behalf.** The documented grammar is
`DX [by <call>] <freq> <call> <remarks>` — the optional `by` clause credits
the spot to a *different* callsign. Through a passthrough that means a
session could put a spot on the network attributed to an arbitrary station,
from a node session authenticated as the shack. If spotting is enabled at
all, **strip or refuse the `by` clause**: a DXCA-originated spot is the
shack's, and attributing it to anyone else is not a facility this needs.
Note too that frequency and callsign may appear in either order
(`DX FR0G 144.600` and `DX 144.600 FR0G` are both valid), so validating a
`dx` line means parsing it properly, not pattern-matching field positions.

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

1. ~~**Plumbing, no auth.**~~ **DONE 2026-08-28.** `cmdrouter.rs` implements
   the queue, the response window and the timers as a pure state machine —
   no sockets, no clock, every entry point taking `now_ms` and returning
   actions, so all of it is testable by feeding it events (10 unit tests).
   `NodeManager` gained `send_line(node, line)` and `subscribe_lines()`, and
   its event loop now publishes the node's own words instead of discarding
   them. Three integration tests prove the round trip against a fake
   DXSpider node. Nothing user-facing changed: no telnet session, no auth,
   and nothing in production subscribes to the new feed yet.

   **One design change this forced.** `LineClass::Prompt` was classified but
   never escaped the client — it paced the init script and stopped there —
   so the router had no completion marker to key on. Added
   `ClientEvent::Prompt(String)`, emitted alongside the existing internal
   handling. The variant is marked `// DXCA:` in the Apache-2.0 Meridian
   module per that file's convention, and it has its own integration test
   because the whole correlation design turns on the event existing.

   **Also settled while building:** the router returns a `consumed` flag
   with its actions, and a consumed event must not flow onward. That is the
   mechanism enforcing the `SH/DX`-must-not-reach-the-pipeline rule from
   §5 — historical spots go to the requester and stop there. It is asserted
   in `sh_dx_results_are_captured_and_never_reach_the_pipeline`, using the
   real spot parser rather than a synthetic value.
2. ~~**Login gate.**~~ **DONE 2026-08-28.** `LOGIN <callsign>` → `Password: `
   → argon2 against the accounts table, off the async runtime via
   `spawn_blocking` because verifying on it would stall every other
   session's spot delivery. Behind `telnet_interactive`, default **false**.
   Anonymous sessions are untouched, which is the whole point and has its
   own test. No passthrough: an authenticated session can currently do
   nothing except `BYE`.

   **Design change from §3.1: login is an opt-in verb, not a prompt on
   connect.** The original text had the server prompt `Login:` when a client
   connects, the way a real node does. That is a guess with a working setup
   as the stake — the loggers on 7575 were configured against a server that
   never prompted, and what they send on connect could not be observed
   without disconnecting a live one. A 45-second capture on the production
   Pi showed an established RUMlog session sending **nothing at all**, which
   rules out mid-session chatter tripping the parser but says nothing about
   connect time. An opt-in verb makes connect-time behaviour irrelevant: a
   client that never sends `LOGIN` cannot be affected. Revisit only with a
   capture of an actual reconnect.

   Two hardening details worth keeping: an unknown callsign still pays for a
   dummy argon2 verification, so response time does not reveal which
   callsigns hold accounts (asserted, including that the dummy hash actually
   *parses* — a malformed one would skip the work and fail silently); and
   `BYE` is honoured only once authenticated, so a logger that happens to
   transmit it is not hung up on.

   **Still open:** the password is echoed by the operator's own terminal.
   Suppressing it needs telnet `IAC WILL ECHO` negotiation, which this
   server does not do (it strips inbound IAC and never negotiates). Since
   v2.3.1 the prompt at least *says* the password will be visible, rather
   than letting it be a surprise.

   **Field bug, fixed in v2.3.1 — "it didn't ask for password".** The
   protocol was working: driving a real telnet client through a pty showed
   the prompt arriving. Three things made it unusable anyway, and all three
   are the kind of defect a passing test suite will never find.

   1. **Nothing on the wire said `LOGIN` existed.** The banner was one line
      about a DX cluster server. An operator connected, watched spots
      scroll, and reasonably concluded nothing had been asked of them. A
      second banner line now says how to log in — only when the gate is on,
      so a plain install's banner is byte-for-byte unchanged.
   2. **The prompt was buried.** `Password: ` deliberately carries no
      newline, because the cursor must stay put for the answer — so on a
      live feed the next spot glued itself to the prompt and scrolled it
      away. The feed is now held **for that one session** while a password
      is outstanding, and resumes immediately after. A few seconds of spots
      are missed; being able to log in is worth more.
   3. **The echo surprise** was left undocumented on screen. Now stated at
      the prompt.

   The lesson worth keeping: every automated test read the socket, where
   the prompt was plainly present. Nothing tested what a person staring at
   a scrolling terminal would actually see, and that is where the feature
   was broken.
3. ~~**Read-only passthrough.**~~ **DONE 2026-08-28.** An authenticated
   session picks a node and issues read-only queries; replies come back to
   it alone. `commands.rs` canonicalizes and tiers, `telnetcmd.rs` holds the
   per-session state and joins policy → router → nodes, and
   `NodeEventFilter` gives the router first refusal on every node event.
   Still behind `telnet_interactive`, still default off.

   **The node runs the canonical form, not what was typed.** `sh/dx 5` is
   forwarded as `SHOW/DX 5`. If the abbreviation went out instead, the
   allowlist would be judging a different string from the one the node
   executes, which is a hole rather than a nicety. Asserted end to end.

   **Interception happens before the status counters, not just before the
   pipeline.** A claimed event is dropped entirely, so a history query does
   not inflate a node's spot count or move its "last spot" clock — the Spots
   screen would otherwise show activity that never happened.

   **The negative test was verified by breaking it.** Removing the
   `set_event_filter` call makes
   `sh_dx_history_reaches_the_asker_and_nothing_else` fail with
   `SH/DX history LEAKED into the spot pipeline: ["VK3XYZ", "ZS6ABC", "K1JT"]`.
   A negative assertion nobody has watched fail is worth very little; this
   one has.

   Refusals name the expansion — `s/pass` is refused *and* told it read as
   `SET/PASSWORD` — so an operator learns why rather than guessing.
   `dangerous_commands_never_reach_the_node` drives real abbreviations
   (`s/pass`, `uns/dx`, `sysop`, `acc/spots`, `dx …`) through a live session
   against a fake node that counts every byte it receives, and asserts the
   count stays zero.

   **First real `SH/DX` against a live node, 2026-08-28.** Manoj ran the
   full sequence on DB0SUE. Everything worked — login, `SH/NODES`,
   `SET/NODE` (lower case accepted), `SH/WWV` returning the real solar
   table, `SH/DX` returning ten historical spots, `S/PASSWORD test` refused
   with *"read as SET/PASSWORD"*, and a clean `BYE`.

   **No history leaked**, and the proof is worth recording because the
   naive check looks alarming: the callsigns from the `SH/DX` reply *are*
   in the spot ring. They arrived legitimately — via **five different
   nodes**, with ages spread 3–10 minutes matching their own `SH/DX`
   timestamps. A leak could only have come via DB0SUE and would have been
   uniformly ~2 minutes old, injected at once.

   Two things the field test taught that the fakes had not:

   - **DXSpider's `SH/DX` replies in a tabular format**
     (`21074.0 VK3AWA 29-Aug-2026 0256Z … <K4ANC>`), not `DX de` lines, so
     those rows never parse as spots anyway. The interception still earns
     its place: a genuinely live `DX de PD2WL:` arrived mid-window and was
     correctly routed to the session — the Spots screen got that DX via
     W3LPL instead, so node redundancy covered the gap.
   - **The reply was interleaved with live spots**, which made the table
     hard to read. Fixed the same evening: the feed is now **held and
     buffered** from the moment a command is submitted until its reply goes
     quiet (2.5 s, capped at 20 s), then flushed — so spots are delayed, not
     dropped. `the_spot_feed_is_held_during_a_reply_then_flushed` covers it,
     and was verified by breaking it.

   **The typing half needed `IAC WILL ECHO`, and now has it (2026-08-29).**
   The operator's own input was shredded by the feed
   (`sh/wwDX de VU2CPL: …v`) because the client echoes locally in line mode:
   it sends nothing until Enter, so the server could not know a line was in
   progress. The server now offers `WILL ECHO` + `WILL SUPPRESS-GO-AHEAD`,
   and a client that accepts switches to character mode — at which point the
   server sees every keystroke, echoes it itself, hides the password by
   simply not echoing it, and holds the feed while a line is part-typed.

   **The offer is made only after `LOGIN` is typed**, never on connect. A
   logger has never received a negotiation byte from this server and still
   does not; `no_negotiation_is_sent_to_a_client_that_never_logs_in` asserts
   it on the banner *and* on the feed. Server echo turns on only when the
   client answers `DO ECHO` — a refusal or silence leaves everything exactly
   as it was.

   **A bug this uncovered, invisible until character mode existed.** RFC 854
   transmits a bare CR as `CR NUL`, and the NUL is padding to discard. It was
   surviving the line split and prefixing the *next* command, so `sh/nodes`
   arrived as `\0sh/nodes` and was refused as an unknown verb. In line mode
   the whole line arrived at once and the stray byte never mattered; the
   moment the client started sending characters, every second command broke.
   Found by driving a real `telnet` through a pty — no unit test would have
   produced `CR NUL`.

   **Enabled on noderedpi4 2026-08-28** and checked against the running
   service: an anonymous session that threw a bare callsign, `set/name`,
   `sh/dx` and `BYE` at port 7575 received **zero** non-spot bytes while
   three real spots flowed through the same socket, and RUMlog reconnected
   by itself after the restart. `LOGIN` prompts; a wrong password is
   refused without saying which half was wrong. **A logged-in `SH/DX`
   against a real node is still unproven** — it needs an account password,
   so it is the operator's to run.

   *Deploy note for anyone repeating this:* `Config` has
   `deny_unknown_fields`, so `telnet_interactive` must go into the TOML
   **after** the new binary is in place. Writing the key first makes the
   running (older) service fail to parse its config on the next restart.
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
- ~~**Does RUMlog send anything on connect?**~~ **Answered enough, 2026-08-28.**
  A 45-second `tcpdump` on the production Pi, filtered to payload-bearing
  packets from the Mac to port 7575, captured **zero** — an established
  RUMlog session is completely silent. Connect-time behaviour remains
  unobserved, because seeing it means disconnecting a live logger. Milestone
  2 sidesteps the question entirely by making login an opt-in verb rather
  than a prompt, so this is no longer a gate on anything. Still worth a
  capture the next time a logger reconnects on its own.

## 9. Appendix — the DXSpider command inventory

Sorted into the tiers of §4 rather than alphabetically, because the tier is
the decision. Sources at the end of this document; the node is always the
authority on what it actually accepts.

**Tier 1 — read-only queries. Safe to forward.**
`show/dx`, `show/mydx`, `show/fdx`, `show/dxcc`, `show/dxstats`,
`show/dxqsl`, `show/wwv`, `show/wcy`, `show/muf`, `show/sun`, `show/moon`,
`show/prefix`, `show/qrz`, `show/qra`, `show/wm7d`, `show/db0sdx`,
`show/heading`, `show/satellite`, `show/contest`, `show/date`, `show/time`,
`show/station`, `show/configuration`, `show/links`, `show/route`,
`show/who`, `show/hfstats`, `show/hftable`, `show/vhfstats`,
`show/vhftable`, `show/files`, `show/filter`, `help`, `apropos`, `dbavail`,
`dbshow`, `who`, `blank`, `echo`, `type`.

*Caveat:* `show/dx`, `show/mydx` and `show/fdx` return `DX de` lines — the
[pipeline-contamination trap](#safety-read-this-before-building).

**Tier 2 — session filters. Forwardable, but they change what the shared
node session receives.**
`accept/spots`, `accept/announce`, `accept/wwv`, `accept/wcy`, `accept/rbn`,
`reject/*` (same set), `clear/spots`, `clear/announce`, `clear/rbn`,
`clear/wwv`, `clear/wcy`, `clear/route`.

**These are not per-user.** DXCA holds *one* session per node, shared by
every DXCA user and by the spot pipeline itself. A filter set through the
passthrough silently narrows what everybody gets, including the Spots feed
and Telegram alerts, and it persists on the node. Recommendation: **refuse
tier 2 initially**, and if it is ever wanted, implement it as a DXCA-side
filter on the operator's own feed rather than a node-side one. This is a
trap the design should not walk into for convenience.

**Tier 3 — transmits. Admin-only, opt-in, off by default.**
`dx` (see the `by`-clause note above), `announce`, `wx`, `talk`, `chat`,
`join`, `leave`, `send`, `reply`.

Note that everything past `dx` in that list is *messaging other humans* as
the shack callsign. None of it is needed for the stated goal, and the
simplest correct answer is to refuse the lot and allow only `dx`.

**Tier 4 — refuse outright. Mutates the account or the session's identity.**
`set/name`, `set/qth`, `set/qra`, `set/location`, `set/address`,
`set/email`, `set/homenode`, `set/password`, `set/startup`, `set/language`,
`set/prompt`, `set/page`, `set/usstate`, `set/dxcq`, `set/dxitu`,
`set/dxgrid`, `set/beep`, `set/echo`, `set/here`, `set/logininfo`,
`set/announce`, `set/talk`, `set/wwv`, `set/wcy`, `set/wx`, `set/seeme`,
every `unset/*`, `unset/privilege`, `sysop`, `kill`, `directory`, `read`,
`enable/ftx`, `disable/ftx`, `bye`.

`set/password` and `sysop` are the obvious ones. But `unset/dx` or
`unset/wwv` would **stop the node sending spots at all** — a single command
that silently kills the shack's feed from that node until someone notices
and re-enables it. `bye` disconnects the shared session. `kill` deletes
mail. None of these belong behind a passthrough, and `bye` in particular
must be intercepted locally as "end *your* telnet session", never forwarded.

## 10. Testing

The queue, terminator detection and allowlist are pure logic and belong in
unit tests with synthesized `ClientEvent` streams — no sockets, no node.
Integration coverage follows the pattern `users_alerts.rs` already uses: a
fake node on a local port, a real telnet session against an ephemeral port,
asserting that a command reaches the fake node, its reply reaches the right
session, and a *second* session sees none of it. That last assertion is the
one that proves the correlation design, and it should exist before the
feature is enabled anywhere.

---

## Sources

- [DXSpider User Command Reference (wiki)](https://wiki.dxcluster.org/wiki/DXSpider_User_Command_Reference)
- [The DXSpider User Manual v1.51 — Command Reference](https://www.dxspider.org/usermanual_en-12.html)
  — abbreviation rule and the `DX` / `SHOW/DX` grammars.
