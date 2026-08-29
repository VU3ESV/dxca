# Phase-rotation spot mask

**Status:** design only, nothing built · **Drafted:** 2026-08-29 ·
**Phase:** 2

Bands rotate through the day. At local midday 160m is dead for anything but
ground wave, and a New-DXCC flag on a 160m spot is an interruption the
operator can do nothing about; at 0200 local it is the most valuable line on
the screen. DXCA currently treats both identically, because it knows nothing
about where the operator is or what time it is there.

Meridian already takes the operator's locator and rotates its skimming bands
by local time. This is the same idea applied to the spot feed: an **optional
mask** that suppresses spots on bands that cannot plausibly be worked from
the operator's QTH right now.

**The single most important design constraint is stated up front, because
everything else follows from it:** the cost of hiding a workable rare one is
far higher than the cost of showing an unworkable one. An operator who
misses a New DXCC because software decided it was daytime will not forgive
the feature, and will not trust the rest of the screen afterwards. Every
decision below is biased accordingly.

---

## 1. What has to be built

Nothing needed for this exists yet. Four pieces, in dependency order:

| Piece | Where | Notes |
|---|---|---|
| Maidenhead → latitude/longitude | `dxca-core` | `wire::looks_like_grid` validates a locator today but nothing converts one |
| Sun elevation for a place and instant | `dxca-core` | Pure math, no data files, no network |
| Band-openness model | `dxca-core` | Elevation → which bands are plausible |
| Per-user locator + mask settings | `dxca-server` + web UI | Rides the existing per-user JSON, so **no migration** |

The first three are pure functions with no I/O, which is the point: they are
testable against published reference values rather than by observation.

## 2. Why sun elevation, not local clock time

The obvious implementation is a table of local-time windows — "160m from
sunset to sunrise" as clock hours. It is wrong at the latitudes and seasons
that matter most.

Sunset in Bengaluru moves by about an hour across the year. Sunset in
northern Europe moves by **six**, and above the Arctic circle the concept
breaks entirely. A fixed clock rule would mask 80m at 1600 in December for a
European operator for whom it has been dark for an hour, and unmask it at
1600 in June when there are five hours of daylight left.

**Sun elevation at the operator's QTH** handles all of that with one
calculation and no tables: it already encodes latitude, longitude, date and
time. The standard NOAA solar-position algorithm is about forty lines,
accurate to well under a degree, and needs nothing but the timestamp and the
coordinates.

Rough windows, to be tuned against real observation rather than treated as
settled:

| Band | Plausible when |
|---|---|
| 160m, 80m | Sun below about −6° (civil twilight or darker) |
| 60m, 40m | Sun below about +5°; best in darkness, usable around dawn/dusk |
| 30m | Always — its distinction is that it works day and night |
| 20m | Sun above about −12°; open well past sunset on long paths |
| 17m, 15m | Sun above about 0° |
| 12m, 10m | Sun above about +10°; needs real daylight and MUF |
| 6m and up | Never masked — sporadic-E and tropo obey none of this |

## 3. What the mask does NOT model

Stated plainly, because a mask that quietly pretends to be a propagation
predictor would be worse than none:

- **Only the operator's end.** Real propagation depends on both ends of the
  path and the ionosphere between them. A 160m opening needs darkness at
  *both* ends; this only knows about one. It is a coarse plausibility
  filter, not a prediction.
- **No solar flux, no K index, no MUF.** DXCA has WWV data available through
  the cluster nodes and deliberately does not use it here. That is a much
  larger feature and it would make the mask's behaviour unpredictable.
- **No grey line.** The most interesting LF propagation happens in the
  narrow window this model treats as a simple threshold crossing.
- **Nothing about antennas.** An operator with no 160m antenna wants that
  band gone at every hour, which is what the existing band chips already do.

## 4. Behaviour — and the safety valves

**Off by default.** An operator who has not set a locator and not switched
the mask on sees exactly what they see today.

**Never silent.** The Spots screen shows a count of what the mask removed —
`14 hidden by band mask` — with a click to reveal. Today's lesson about the
alert-level filter applies directly: a filter that silently empties the
screen reads as a broken feed, and the operator has no way to tell the
difference. The count is what makes the mask honest.

**Two modes, and the softer one is the default:**

- **Dim** — masked spots stay in the table, visually receded. Nothing is
  hidden, the operator's eye simply skips them. This is the recommended
  default because it cannot cost a contact.
- **Hide** — masked spots are removed from the list. Cleaner on a busy feed,
  and the mode to choose deliberately.

**A floor on alert level.** A setting for "never mask at or above this
level", defaulting to **New DXCC**. The rarest catch always shows, whatever
the sun is doing, because that is precisely the spot worth breaking the rule
for — and it is the case where being wrong is unforgivable.

**Telegram narrows separately**, exactly as the band/mode and manual-only
narrowings already do: `notify_respect_band_mask`. Watch everything on
screen, be woken only for what is workable.

## 5. Where the computation lives

Server-side, annotating each spot with a `band_open: bool` as it is
classified — the same shape as `is_skimmer` and for the same reasons. One
implementation feeds both the UI and the Telegram fan-out, so the two can
never disagree about what is masked, and the browser needs no solar maths.

The annotation is computed at classification time and not revisited. A spot
list spans a few minutes; the sun does not move enough in that window to
matter, and re-deriving it per render would be motion without meaning.

**Time comes from the server clock in UTC.** On this shack that is a
GPS-disciplined Pi, so it is trustworthy; on someone else's install it is
whatever NTP they have. A clock hours out would mask the wrong bands, which
is worth a line in the docs but not a defensive mechanism.

## 6. The locator

A per-user field, 4 or 6 characters, validated with the existing
`looks_like_grid`. It lives in the per-user JSON blob, so **no schema
migration** — an account without one simply has the mask unavailable.

**Offer to prefill it from the ClubLog log.** The ADIF carries
`MY_GRIDSQUARE` on the operator's own QSOs, and `adif.rs` already parses
grid squares. Reading the most common value from their own log and offering
it saves a lookup, and an operator who has never typed their locator into
DXCA has certainly typed it into their logger.

**6-character precision is pointless here** and 4 is plenty: a grid square
is 70 by 100 miles, and sunset differs across it by a few minutes. Accept
both, use what is given, do not ask for more.

## 7. Milestones

1. **Maidenhead + solar, pure.** `grid_to_latlon` and `sun_elevation`, with
   golden tests against published values — JN58TD is Munich; NOAA publishes
   sunrise and sunset for known places and dates. Nothing user-facing.
2. **The band model and the locator field.** `band_plausible(band, elev)`,
   the per-user setting, and the API annotation. Still nothing visible.
3. **Dim mode, with the masked count.** The Spots screen only. This is the
   milestone worth stopping at to live with for a week before going further.
4. **Hide mode and the Telegram narrowing.** Only once the model has been
   watched against real conditions and the windows in §2 have been tuned.

Stopping after 3 is a perfectly good outcome, and the tuning that milestone
produces is worth more than the code in milestone 4.

## 8. Open questions

- **Should the mask consider the DX end at all?** Suppressing 160m when the
  path is entirely sunlit would be a real improvement and needs the spotted
  station's location — which DXCA already resolves to a DXCC entity, though
  an entity centroid is a crude proxy for a station's actual position.
- **What tunes the windows in §2?** The honest answer is a week of watching
  the mask against a real feed and moving thresholds where it disagrees with
  the operator. Worth building a "would have masked" debug view first, so
  the tuning can happen without anything being hidden.
- **Does 60m belong with 40m?** Its propagation sits between 80 and 40, and
  it scores nothing for the Challenge anyway.
