# Award filters and the unconfirmed-alert gate

**Status:** phase 1 (the unconfirmed-alert gate) **built, on main,
unreleased** — to be tried on the local Pi alongside the TCI destination
before any tag. Phases 2–4 (VUCC / IOTA / WAS) remain design only ·
**Drafted:** 2026-09-01 · **Targets:** dxca ≥ 2.16

Two related feature requests from VU2CPL, 2026-09-01:

1. An `Unconf*` alert today fires for **every** spot of a worked-but-
   unconfirmed entity — including the very station that never QSLed the
   first QSO. Some operators simply refuse to QSL; re-working them wastes
   time. The alert is only worth the interruption for a **new call that
   uses LoTW** — a station that can be worked *and* will confirm.
2. Beyond DXCC, track **IOTA**, **WAS** and **VUCC** as selectable awards,
   each with its own needed/worked/confirmed state — which means spots
   need IOTA / grid / state fields, and the log matrix needs axes other
   than the DXCC entity id.

They phase naturally: the gate needs **no new data** and ships first; the
awards need new data plumbing and land in three further phases.

---

## 1. Phase 1 — the unconfirmed-alert gate

Two independent per-user toggles, applying **only to the four `Unconf*`
levels** (`UnconfDxcc/Band/Mode/Slot`, `classify.rs:35-42`). The `New*`
levels are untouched on purpose: an ATNO is worth working whatever the QSL
prospects — paper still exists.

| Toggle | Suppresses an `Unconf*` alert when… | Data source (all existing) |
|---|---|---|
| `notify_unconf_skip_worked` | the DX call is already in the user's log | `LogMatrix.worked_calls` — populated at `matrix.rs:154`, **never consumed anywhere yet**; this is its first real user |
| `notify_unconf_lotw_only` | the DX call is not a LoTW user | `UserService::is_lotw_user()` (`users.rs:88-90`), backed by the weekly-refreshed `data/lotw-users.txt` |

Both default **off** (a new account behaves exactly as today). Both on =
the request verbatim: only a never-worked LoTW user alerts on an
unconfirmed entity.

### Where it goes

`UserService::fan_out` (`users.rs:329-441`) is the single per-user
decision chain — the gate slots in immediately after
`notify.wants_level(c.level)` (`db.rs:441`):

```text
wants_level → [NEW: unconf gate] → passes_band_mode → passes_spotter
            → passes_band_mask → cooldown
```

Needs a small `AlertLevel::is_unconfirmed()` helper (none exists today).

### Call matching

`worked_calls` holds exact logged calls, lowercased. A spot may carry
`VP8/K1ABC` where the log has `k1abc`. Mirror `lotw::is_user`'s slash
handling (`lotw.rs:49-66`): check the exact call, the segment before `/`,
and the segment after `/`.

### Plumbing and scope

- `NotifyUserConfig` (`db.rs:203`) rides the per-user JSON — two new
  `#[serde(default)]` bools, **no migration**, saved via the existing
  `PUT /api/config/me/notifications`.
- UI: two toggles in the Alerts-screen rail, grouped under the Unconf
  levels (`Alerts.svelte`).
- Gates **Telegram / Flex / TCI alerts only.** The Spots screen keeps
  showing the level dot (display is a different question), and the
  telnet/UDP/MQTT feeds stay deliberately unfiltered.
- The per-call cooldown already prevents repeat alerts; this gate is
  about *never* alerting for hopeless calls, not rate-limiting them.

### Deferred refinement — proven QSLers

`worked_calls` skips *everyone* already worked — including a station that
confirmed you on another band, who would very likely confirm the missing
one too. The sharper rule is "skip only calls that are worked AND have
never confirmed me", which needs a sibling `confirmed_calls` set built in
the same ADIF pass (`Record::is_confirmed()`, `adif.rs:87-96`). **v1
ships the strict rule** (matches the request as stated); the refinement
is a follow-up if the strict rule proves too aggressive.

---

## 2. Phases 2–4 — IOTA / WAS / VUCC award tracking

### 2.1 Data reality (researched 2026-09-01)

| Award | Reference data | Log side (worked/confirmed) | Spot side |
|---|---|---|---|
| DXCC | cty.xml (existing) | ClubLog (existing) | existing |
| VUCC | grid math already in `grid.rs` | ClubLog **stores** `GRIDSQUARE`/`VUCC_GRIDS` per its field doc — verify one real `getadif.php` export actually emits it; LoTW report as backstop | grid is **already parsed** from cluster comments (`wire.rs:35-59`) and dropped in `synthetic_spot()` (`nodes.rs:292-335`); FT8 CQ messages carry it in the message text |
| IOTA | iota-world.org `fulllist.json` / `groups.json` / `islands.json` — free JSON downloads, "personal non-commercial" terms → **download at runtime like cty.xml, never bundle in the repo** | LoTW report (IOTA accepts LoTW QSO matching since 2020); **not ClubLog** — Club Log discards the `IOTA` field on upload | regex over cluster comments: `\b(AF|AN|AS|EU|NA|OC|SA)-\d{3}\b`; later, the iota-world "accepted activations" list (call + ref + dates) could tag spots whose comment omits the ref, but it is published as a PDF — fragile, defer |
| WAS | static 50-state table | LoTW report — WAS is LoTW-administered; **not ClubLog** (discards `STATE`) | **the hard one**: nothing on the wire carries state; needs FCC ULS distilled to a call→state table |

Key sources: Club Log field list
(<https://clublog.freshdesk.com/support/solutions/articles/53202>),
IOTA downloads
(<https://www.iota-world.org/islands-on-the-air/downloads.html>).

### 2.2 New log source: a LoTW report client

One addition unlocks the confirmed side of all three new awards:
`https://lotw.arrl.org/lotwuser/lotwreport.adi` with the user's LoTW web
login (`qso_qsl=yes&qso_qsldetail=yes`, incremental via `qso_qslsince`).
QSL detail records carry `STATE`, `GRIDSQUARE` and `IOTA` where the
confirming station's TQSL location provides them. Per-user credentials
stored alongside the ClubLog ones in the per-user JSON; refreshed on the
same cadence as the ClubLog matrix. ClubLog remains the DXCC/worked
source of truth — LoTW is additive, for the fields ClubLog cannot have.

### 2.3 Spot model additions

`Spot` (`spot.rs:9-80`) gains `grid`, `iota`, `state` as
`Option<String>`, populated at ingest:

- **grid** — stop dropping `ParsedSpot.grid` in `synthetic_spot()`; also
  extract the trailing 4-char grid from FT8 CQ message text.
- **iota** — comment regex above, validated against the downloaded
  directory.
- **state** — call→state lookup, US calls only.

All three flow to the web UI via `annotate_spot()` and stay **out of**
the DX-Spider wire format (downstream loggers get the standard line).

### 2.4 Matrix and classifier

`LogMatrix.by_dxcc` (`matrix.rs:27`) stays untouched. New parallel maps,
same worked/confirmed shape, different keys:

- IOTA: ref → worked/confirmed (ref-level; band endorsements deferred)
- WAS: state → worked/confirmed per band × mode-class (basic mixed first)
- VUCC: 4-char grid → worked/confirmed **per band, 50 MHz and up only**
  (decided 2026-09-01; needs an `is_vucc_band()` helper in `bands.rs` —
  6M and up, `Sat` still out of scope per HANDOVER)

Classifier: per-award `New*`/`Unconf*` pairs (e.g. `NewIota`,
`UnconfGrid`) appended to `AlertLevel::FLAGGABLE` and served through
`GET /api/reference` so the UI chip groups pick them up without drift.
An award only classifies when the user has enabled it — an **award
selector** (per-user, in Settings) plus award chip groups on the Spots
and Alerts rails. The Phase-1 gate applies to the new `Unconf*` levels
identically.

### 2.5 Phasing

| Phase | Ships | New data needed |
|---|---|---|
| 1 | Unconf gate (skip-worked + LoTW-only) | none |
| 2 | VUCC: grid on spots, grid matrix, 50 MHz gate | verify ClubLog export emits `GRIDSQUARE` |
| 3 | IOTA: directory download, comment parsing, LoTW report client | iota-world.org JSONs; LoTW credentials |
| 4 | WAS: state matrix, call→state table | FCC ULS distillation (size/cadence to be decided — the weekly full dump is ~200 MB raw; distilled call→state is a few MB) |

### 2.6 To verify before building

1. One real `getadif.php` download → does the export include
   `GRIDSQUARE`? (Club Log stores it; storing ≠ exporting.)
2. Eyeball `fulllist.json` against its structure PDF.
3. FCC ULS distillation: produce the call→state file once by hand,
   measure size, then decide refresh cadence and where it runs (the Pi
   should not download 200 MB weekly).
