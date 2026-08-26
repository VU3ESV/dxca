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
Aggregator); rewritten and extended. The DX-cluster telnet engines derive
from [Meridian](https://github.com/thomasbasil/meridian).

## Status

**M5 core complete — the dashboard is live** (2026-08-27): dxca is the
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
artifacts). Remaining: web editing of the global config (M5 remainder),
then M6 — Pi cutover, systemd, v2.0.0. The full design and milestone
plan: [docs/PLAN.md](docs/PLAN.md). The 1.x macOS app remains the
standing fallback until M6 signs off.

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

### Pi build

Cross-compile from any machine with
[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)
(`brew install cargo-zigbuild`) and the target
(`rustup target add aarch64-unknown-linux-gnu`):

```sh
just dist    # → target/aarch64-unknown-linux-gnu/release/dxca
```

The binary targets glibc ≥ 2.36 (Raspberry Pi OS Bookworm, 64-bit). Copy
it over together with a `config/dxca.toml`; systemd packaging arrives in
M6.

## Configuration

Global (admin) settings live in `config/dxca.toml` — see the committed
[example](config/dxca.example.toml). Defaults keep the 1.x ports: web GUI
**7580**, telnet cluster server **7575**, WSJT-X UDP listen **2237**.
Per-user settings (ClubLog credentials, alert preferences, Telegram) are
managed in the web GUI per account, not in the file.

## License

MIT — see [LICENSE](LICENSE).
