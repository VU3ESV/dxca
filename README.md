# DXCA

FT8/FT4 + DX-cluster spot aggregator with a multi-user web GUI. Rust
successor to [DXClusterAggregator for
macOS](https://github.com/vu2cpl/DXClusterAggregator-macOS), built to run
24/7 on a Raspberry Pi (equally at home on macOS/Linux/Windows).

DXCA ingests spots from WSJT-X/JTDX instances (binary UDP) and DX-cluster
telnet nodes, aggregates and dedupes them, and serves the result to logging
software over a built-in telnet cluster server and to UDP destinations
(including a verbatim passthrough that keeps loggers' click-to-fill
working). A web GUI with per-user accounts lets each operator carry their
own ClubLog log matrix, New-DXCC/Slot/Band/Mode highlighting, and Telegram
alerts over one shared spot stream.

Original concept and reference implementation by Vinod VU3ESV (FT8 Cluster
Aggregator); rewritten and extended. The DX-cluster telnet client engine
is derived from the **Meridian** project — joint work by Basil Thomas
W6BT, Vinod VU3ESV, and Ram VU3RDD (repo private).

## Status

**v2.0.0 — M6** (2026-08-27): dxca is the
live shack aggregator with a real web GUI. Decoder UDP sources and five
DX-cluster telnet nodes (Meridian-lifted client with the 1.x
honest-status graft) feed one pipeline into the telnet server, the RUMlog
passthrough, and a WebSocket-streamed spots dashboard — status pills,
live sortable table, source/band/new-only/duplicate filters, per-user
alert row tints, LoTW markers. SQLite-backed accounts (argon2 + session
cookies, first-run setup card) each carry their own ClubLog matrix;
every spot classifies per user with Telegram alerts and per-callsign
cooldown. Proven end-to-end in tests against fake ClubLog/Telegram
servers and by the live validations along the way (RUMlog click-to-fill,
honest-yellow flaky node, exact matrix parity with the 1.x app's own
artifacts). Sources, cluster nodes, and broadcast destinations are
edited in the System tab and hot-apply — listeners rebind, nodes redial,
destinations re-point, and `config/dxca.toml` is rewritten so restarts
agree. M6 packaged it: launchd agent on the Mac, systemd service on
noderedpi4 (installed and standing by for the decoder cutover — see
HANDOVER for the checklist). The full design and milestone plan:
[docs/PLAN.md](docs/PLAN.md). The 1.x macOS app is the standing
fallback.

Secrets note (plan §5): per-user ClubLog app passwords and Telegram
tokens live in `data/dxca.db` in plain text, file mode 0600, service user
only — encryption-at-rest on the same host would add ceremony, not
security. Keep the data directory out of backups you share.

## Layout

| Path | What |
|---|---|
| `crates/dxca-core` | Pure logic: spot model; WSJT-X codec, parsers, matrix, classifier land in M1. No I/O. |
| `crates/dxca-connect` | I/O engines (M2–M4): DX-cluster telnet, WSJT-X UDP, broadcaster, ClubLog/LoTW/Telegram. |
| `crates/dxca-server` | Composition root: config, axum web API, embedded UI; auth + SQLite in M4. Binary is `dxca`. |
| `web-ui/` | Svelte 5 + Vite + TypeScript (pnpm). Built `dist/` is embedded into the binary. |
| `config/dxca.example.toml` | Global config template — copy to `config/dxca.toml`. |

## Build

Needs Rust (stable, via rustup) and — only for the web UI — Node ≥ 20 with
pnpm. Plain `cargo build` never requires Node (a stub page is embedded when
`web-ui/dist` hasn't been built).

```sh
cargo build --workspace          # all crates
cargo test --workspace
pnpm -C web-ui install && pnpm -C web-ui build   # real web UI into dist/
cargo run -p dxca-server         # http://localhost:7580
```

A [Justfile](Justfile) wraps the common flows (`just gate`, `just run`,
`just dist`) but is never required.

### Install as a service

`./install.sh` sets the current machine up (auto-detects macOS vs
Raspberry Pi, confirms, never fails silently):

- **macOS**: builds the release binary and installs a launchd agent
  (`com.vu2cpl.dxca`, survives reboots, log in `~/Library/Logs/dxca.log`).
- **Pi/Linux**: installs binary + config + data seeds to `/opt/dxca` and
  a systemd service (`systemctl status dxca`), running as the invoking
  user. A fresh install self-bootstraps: the first-run web card creates
  the admin account, and cty.xml / the LoTW list download on demand once
  a ClubLog API key is entered — no seed files required.

To cross-compile on the Mac and ship to a Pi in one step
(needs cargo-zigbuild + the `aarch64-unknown-linux-gnu` target):

```sh
deploy/pi-deploy.sh vu2cpl@noderedpi4.local
```

The aarch64 binary targets glibc ≥ 2.36 (Raspberry Pi OS Bookworm+,
64-bit). Existing config and data on the Pi are never clobbered —
`install.sh` seeds them only when absent.

## Configuration

Global (admin) settings live in `config/dxca.toml` — see the committed
[example](config/dxca.example.toml) — and, once the server runs, in the
web UI's System tab (hot-applies and rewrites the file). Defaults keep
the shack wiring: web GUI **7580**, telnet cluster server **7575**,
decoder sources MSHV **2333** / JTDX **2334** / WSJTX **2335**,
passthrough → RUMlog **2237**.
Per-user settings (ClubLog credentials, alert preferences, Telegram) are
managed in the web GUI per account, not in the file.

## License

MIT — see [LICENSE](LICENSE) — except
`crates/dxca-connect/src/dxcluster/`, which is derived from the Meridian
project (© the Meridian authors: Basil Thomas W6BT, Vinod VU3ESV, Ram
VU3RDD) and remains under **Apache-2.0** — see
[LICENSE-APACHE](LICENSE-APACHE). DXCA's modifications to those files
are marked `// DXCA:`.
