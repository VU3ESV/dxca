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

**M1 in progress** (2026-08-26) — M0 scaffold done (workspace, embedded-web-UI
server stub, local gate, Pi cross-compile verified on hardware); the WSJT-X
binary codec is ported and golden-tested against datagrams captured from
live MSHV/JTDX/WSJT-X instances (`crates/dxca-core/tests/vectors/`). No
spot pipeline yet. The full design and milestone plan:
[docs/PLAN.md](docs/PLAN.md). The production DXCA remains the macOS 1.x app
until M6 signs off.

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
