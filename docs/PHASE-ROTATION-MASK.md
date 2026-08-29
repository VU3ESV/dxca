# Phase-rotation spot mask

**Status:** **all four milestones built and verified in a browser.** The
maths, the band model, the per-user locator, the API annotation, dim mode
with its count, hide mode, and the Telegram narrowing. The model now runs on
**sun phases around a configurable grey-line window** rather than fixed
elevation thresholds ·
**Drafted:** 2026-08-29 · **Phase:** 2

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

## 2. Why sun phases, not clock time and not raw elevation

The obvious implementation is a table of local-time windows — "160m from
sunset to sunrise" as clock hours. It is wrong at the latitudes and seasons
that matter most. Sunset moves about an hour across the year in Bengaluru
and **six** in northern Europe; above the Arctic circle the concept breaks
entirely.

The first implementation therefore used **sun elevation**, which encodes
latitude, longitude, date and time in one number. That was right about day
and night and wrong about the thing that matters most on the low bands: the
**grey line**, the narrow window either side of the terminator where the D
layer has collapsed but the F layer is still lit. An elevation threshold
cannot express it, because a fixed number of degrees is a wildly different
amount of *time* depending on where you are — the sun sets almost vertically
at the equator and crawls at 48°N. Measured, at the June solstice, 45 minutes
before sunset is about 9° up in Bengaluru and about 5° in Munich.

So the model resolves **phases against the real sunrise and sunset** for that
place and day:

| Phase | When |
|---|---|
| **Dawn** | within the window either side of sunrise |
| **Day** | between the two windows, sun up |
| **Dusk** | within the window either side of sunset |
| **Night** | between the two windows, sun down |

and the **window is the operator's to set**, defaulting to **45 minutes**.
How long the grey line stays useful genuinely varies — with the band, the
season, the path and the station — so this is a number to nudge and watch,
not a constant to get right once. This is Meridian's model, defaults
included, so the two programs cannot disagree about what phase it is.

Which bands are plausible in which phase:

| Band | Dawn | Day | Dusk | Night |
|---|:-:|:-:|:-:|:-:|
| 160m, 80m, 60m, 40m | ● | | ● | ● |
| 30m | ● | ● | ● | ● |
| 20m | ● | ● | ● | ● |
| 17m, 15m | ● | ● | ● | |
| 12m, 10m | | ● | | |
| 6m and up | ● | ● | ● | ● |

The grey-line columns are where the low bands and the high bands are open at
the same time, which is exactly the overlap a single elevation threshold
could not produce. 30m and 6m-and-up have no entry at all and are therefore
never masked.

Polar day and polar night have no sunrise or sunset to be either side of;
they return Day and Night for the whole 24 hours. When a short day or night
makes the two windows overlap, they meet at solar midday/midnight and the
vanishing Day/Night is never returned — which is the correct answer at high
latitude in June, where the whole night *is* grey line.

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
- **The grey line is a window, not a physical model.** Dawn and Dusk are
  "within N minutes of the terminator", which is a decent proxy and not
  ionospheric physics. It does not know that the useful window is longer on
  160m than on 40m, or that it stretches along the terminator rather than
  around the clock.
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

**The grey-line window lives beside the locator**, as a stepper defaulting
to 45 minutes, bounded to 5–360 and **refused rather than clamped** outside
that: silently changing a number the operator typed is how they stop trusting
the screen. Below 5 minutes the grey line is too narrow to be a phase; above
360 it stops being a grey line and starts being most of the day, at which
point the mask is saying nothing.

**6-character precision is pointless here** and 4 is plenty: a grid square
is 70 by 100 miles, and sunset differs across it by a few minutes. Accept
both, use what is given, do not ask for more.

## 7. Milestones

1. ~~**Maidenhead + solar, pure.**~~ **DONE 2026-08-29.** `grid::parse`
   returns the **centre** of a 4- or 6-character square (the corner is up to
   75 km out and the centre costs nothing), and `solar::elevation` is the
   NOAA solar-position algorithm — no data files, no network, no clock of
   its own.

   **Validated against the outside world, not only against itself.**
   Computed sunrise agrees with published times to within 5–8 minutes, in a
   consistent direction: Munich in June 03:20 UTC against a published 03:14,
   in December 07:08 against 07:03, Bengaluru in June 00:32 against 00:24.
   That bias is **atmospheric refraction** — almanacs quote the *apparent*
   sunrise, this returns the true geometric one — and it is left uncorrected
   deliberately, because no band-openness threshold can use that precision.
   The test pins the bias with its explanation, so a future reader finds the
   reason rather than a mystery.

   The polar cases have their own tests, since they are the whole argument
   for elevation over clock time: at Tromsø the sun never sets across all 24
   hours in June, and never rises in December. A local-time rule would be
   wrong there for months at a stretch.

   One correction worth recording: MK68 is **not** Bengaluru — it is 18.5N
   73.0E. Bengaluru is MK82. Transposing the two square digits moves you
   several hundred kilometres, and both are now pinned in the tests.
2. ~~**The band model and the locator field.**~~ **DONE 2026-08-29.**
   `bands::plausible_at(band, elevation)` implements the §2 table and
   **fails open** — an unknown band, or one the model says nothing about
   (30M, 6M and up), is always plausible. That default is the asymmetry at
   the top of this document expressed in code: ignorance must never mask.

   The locator lives in a new `station_json` per-user blob, added to
   `user_configs` through the migration mechanism built earlier the same day
   — its own blob rather than a field on the ClubLog credentials, because a
   locator is station data and will gain company as the mask grows.
   `PUT /api/config/me/station` **validates** it and rejects a typo with a
   message naming the format, rather than accepting it and silently
   disabling the mask: an operator who sets a locator and sees nothing
   happen cannot tell a rejected value from a broken feature.

   Spots now carry `band_open` — but **only** when the account has a valid
   locator, and it is advice the server never acts on. Nothing is withheld,
   nothing is dimmed; the client decides, which is what keeps the feature
   opt-in and default-off. `no_locator_means_no_band_annotation` asserts
   exactly that, including that an unparseable locator behaves as none.

   Computed **once per request**, not per spot — the sun does not move
   across a spot list, and a config read per row would be absurd. The
   WebSocket recomputes per frame instead, because a session can stay open
   across a sunset and a stale elevation would mask the wrong bands for the
   rest of the evening.
3. ~~**Dim mode, with the masked count.**~~ **DONE 2026-08-29.** The Spots
   screen only, and the milestone worth living with for a week before going
   further.

   A **Band mask** tickbox sits with the other display narrowings, off by
   default, remembered per browser in `localStorage`. It appears **only once
   a locator is set** — without one the server sends no advice, and a
   permanently dead checkbox is worse than no checkbox. The route to it is
   the note beside the Locator field in Settings › My station, which names
   the feature.

   Masked rows are **dimmed to 45% and restored in full on hover**. That
   hover rule is the safety valve made physical: a receded row is always one
   pointer away from being read, so the mask cannot turn a workable spot
   into a puzzle. Opacity rather than a muted colour, deliberately, because
   it fades the alert tint too — a New Band flag on a dead band should look
   like a quiet flag rather than a loud one.

   The mask takes **no part in the `visible` filter**. It cannot empty the
   table, cannot interact with the other narrowings, and cannot produce the
   "screen went blank" failure the alert-level filter taught us about. The
   `N dimmed` badge beside the spot count is what keeps it honest.

   **One real bug surfaced while verifying it, and it was not in the mask.**
   `band_open` was annotated inside the classification branch, and
   `users::classify` returns `None` for an account with no ClubLog matrix —
   so the mask's precondition had silently become *"has a ClubLog log"*
   rather than *"has a locator"*, and the Band column was empty for those
   accounts too. A band is a property of the spot's frequency, not of
   anyone's log. `annotate_spot` now derives it from the frequency
   unconditionally, and classification only adds the alert level, DXCC name
   and beacon flag on top.

   Verified against the real pipeline rather than by reading: a local
   instance with **no cluster nodes** (logging in as VU2CPL would fight the
   shack Pis for the same node session) fed synthetic WSJT-X UDP packets on
   four bands. At 11:26 IST from MK82 the 160M and 40M rows dimmed and the
   15M and 10M rows did not, the badge read `5 dimmed`, hover restored a
   row, both themes were legible, and the preference survived a reload.

   **Not yet exercised on a real feed:** the New DXCC exemption. It needs a
   loaded ClubLog log to produce a flagged spot, so the local check could
   not reach it — the logic is a one-line list membership, but it is
   untested against live data.
4. ~~**Hide mode and the Telegram narrowing.**~~ **DONE 2026-08-29**, at
   Manoj's instruction and earlier than this plan intended — the plan said
   to watch the thresholds for a week first. What made that safe to skip is
   that the thresholds themselves stopped being the fragile part: the §2
   rewrite moved the tuning knob out of the source and into the operator's
   hands as the grey-line window, so a model that disagrees with the bands
   is now something the operator adjusts rather than something they wait for
   a release to fix.

   **Hide mode** removes masked rows from the list. It is a `<select>` beside
   the tickbox, it appears only once the mask is on, and **dim stays the
   default** — a corrupted or half-written preference lands on dim, never on
   hide. Crucially the `N hidden` count is derived from the rows *before*
   hiding, so the number survives the thing it counts; a mask that removed
   rows and lost count of them would be exactly the silent-filter failure
   this feature exists to avoid.

   **The Telegram narrowing** is `notify_respect_band_mask`, off by default
   and narrowed separately from the screen, like every other Telegram
   narrowing. Two rules matter more here than on screen, because a held
   alert is a spot the operator never learns about at all rather than one
   they can hover:

   - **New DXCC is exempt**, and the tickbox says so on its own label
     rather than in a tooltip — it is the reassurance that makes the setting
     safe to enable, and a reassurance nobody reads is not one. The screen
     never dims it; Telegram never holds it. If the model is ever wrong, being wrong about the rarest catch of
     the year is the one failure that would end this feature's welcome.
   - **No opinion never suppresses.** No locator, or a band the model says
     nothing about, sends as it always did. `telegram_band_mask_fails_open`
     pins all four cases.

   The phase is computed per spot in the fan-out rather than cached, because
   the fan-out runs continuously and a phase read at startup would narrow the
   wrong bands for the rest of the evening.

All four are built. The tuning milestone 3 was meant to produce is now the
operator's dial rather than a source edit, which is the better outcome.

## 8. Open questions

- **Should the mask consider the DX end at all?** Suppressing 160m when the
  path is entirely sunlit would be a real improvement and needs the spotted
  station's location — which DXCA already resolves to a DXCC entity, though
  an entity centroid is a crude proxy for a station's actual position.
- ~~**What tunes the windows in §2?**~~ **Answered by the phase rewrite:**
  the operator does, with the grey-line window, without waiting for a
  release. What remains untunable is the band-to-phase table itself —
  Meridian solves that with per-band phase checkboxes, and DXCA could grow
  the same thing if the fixed table turns out to be wrong for a real
  station.
- **Does 60m belong with 40m?** Its propagation sits between 80 and 40, and
  it scores nothing for the Challenge anyway.
