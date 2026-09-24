# DXCA

Rust workspace + Svelte 5 UI embedded into the binary at compile time
(`include_dir` over `web-ui/dist`), so **build the web UI before the binary** or
you ship the placeholder page.

`dxca-core` no I/O · `dxca-connect` I/O engines, never imports axum/SQLite/auth ·
`dxca-server` composition root, API, DB.

## Gate

`just gate` (= fmt check, clippy `-D warnings`, `cargo test --workspace`, web
build) before every commit. Invoking cargo directly needs
`PATH=/opt/homebrew/opt/rustup/bin:$PATH` — rustup here is keg-only and `cargo
fmt`/`clippy` are otherwise "no such subcommand". The Justfile does this; a bare
shell does not.

## Traps

**The macOS launchd agent runs `target/release/dxca` from this clone.** A
`cargo build --release` on a branch arms it to run unreleased code at its next
restart. Defused for now — disabled on this Mac since 2026-09-13 (`launchctl
disable`, which persists where the earlier `bootout` did not) and production has
been the .109 container since 2026-09-22 — but `install.sh macos` bootstraps the
agent again, and one `launchctl enable` brings the hazard straight back. Use a
git worktree for branch work that compiles, or rebuild from `main` after.

**`notify_json` is replaced wholesale.** Telegram, Alerts, FlexRadio and TCI all
edit that one row. Each page must load the whole object and write it back with
only its own fields changed — a partial PUT silently clears the others.

**Every new `NotifyUserConfig` field needs an explicit serde default** plus a
test pinning what an old stored row without the key reads as — every existing
install predates every new field. Usually `#[serde(default)]`, i.e. off; the
award levels use `#[serde(default = "default_true")]` on purpose, since opting
an award in should not need a second opt-in for its pings. See the
`*_defaults_off_for_a_stored_row_that_predates_it` tests in `db.rs`.

**The ClubLog key is baked in at build time and never committed.** `build.rs`
reads `DXCA_CLUBLOG_API_KEY`, or `.clublog-api-key` at the repo root (gitignored),
and writes it into `OUT_DIR`. This repo is public and Club Log delete keys they
find published, so it stays out of the source tree — never fold it back into a
constant. A build with neither source present gets an empty key and fails
quietly: no country file, nothing classifies, and nothing says why.

## Adding a spot destination

Touches seven places; miss one and it looks wired and does nothing:

`dxca-connect/src/<name>.rs` · `dxca-connect/src/lib.rs` (`pub mod`) ·
`db.rs` (`NotifyUserConfig` fields **+ `Default`**) · `users.rs` (client map,
`push_<name>`, `fan_out` gate) · `settings/<Name>.svelte` ·
`settings/Destinations.svelte` (register tab) · `README.md`

`flex.rs` (TCP) and `tci.rs` (WebSocket) are the two worked examples.

## House style

Comments carry the **why**: the rejected alternative, the failure mode being
guarded, the protocol trap. Tests get a doc comment naming the regression they
prevent. A comment restating the code is worse than none. Read `tci.rs` before
writing a new module — matching that voice is part of the diff.

Deps are added reluctantly and pinned once in the root `[workspace.dependencies]`.

## Where things are written down

`HANDOVER.md` — engineering log, incl. what was tried and rejected, opening with
a contents index that a new section owes an entry —
`scripts/check-handover-index.py` verifies it. `README.md` — user-facing
manual. Both are long; grep them, don't read them whole.
