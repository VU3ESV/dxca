<script lang="ts">
  // The spots dashboard: status pills + live table (plan §8 page 2).
  // Backfills over /api/spots, then rides /api/stream; filters and the
  // 60 s duplicate collapse mirror the 1.x display behaviour.
  import { api, openStream, hhmm, ago } from '../lib/api';
  import { onMount } from 'svelte';
  import ChipGroup from '../lib/ChipGroup.svelte';
  import FilterRail from '../lib/FilterRail.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { setStatus } from '../lib/status.svelte';
  import { awards, pick, canFilter } from '../lib/awards.svelte';
  import { bandMask, masked, hidden } from '../lib/bandmask.svelte';
  import { loadReference, bands, modes, levels, levelLabel } from '../lib/reference.svelte';
  import { loadChase, chasedLevels } from '../lib/chase.svelte';

  let spots = $state<any[]>([]);
  let status = $state<any>(null);
  let station = $state<any>(null);
  /// The operator's Maidenhead square, or '' — the band mask's precondition.
  /// Asked for directly rather than inferred from whether spots carry
  /// `band_open`, because an empty feed or a batch of unclassified spots
  /// would make a configured locator look absent and hide the control.
  let locator = $state('');
  /// Where the sun is for this account: phase, sunrise, sunset. Null until
  /// loaded, and stays null for an account with no locator — the server
  /// answers 204 for that, which is the normal case, not a failure.
  let sun = $state<any>(null);
  let sortKey = $state('time_unix');
  let sortDesc = $state(true);
  /// Free-text narrowing, matched against the spotted call and the spotter.
  /// Deliberately not persisted: a forgotten search that survives a reload
  /// looks exactly like a broken feed.
  let search = $state('');
  /// Who did the spotting. A skimmer's `-#` marker is stripped off the
  /// callsign, so without the server's flag `W3LPL` and `W3LPL-#` are
  /// indistinguishable here — and they are not the same kind of spot.
  ///
  /// Three-way rather than the old "Manual only" tick, which could only ever
  /// take skimmers AWAY. Skimmers are most of the feed on a busy band, so
  /// "show me only what the machines heard" is as real a question as its
  /// opposite — a CW skimmer sweep is exactly where a rare prefix surfaces
  /// first, and there was no way to ask for it.
  let spotterKind = $state<'all' | 'human' | 'skimmer'>('all');
  let cqOnly = $state(false);
  let hideDupes = $state(true);
  let sourceFilter = $state<Set<string>>(new Set());
  const MAX_ROWS = 1500;

  // The display narrowing. Empty = everything, matching the server's own
  // convention for the Telegram lists. These are the SPOTS screen's own —
  // deliberately independent of the Telegram narrowing, so the operator can
  // watch the whole band plan while being pinged for one slice of it.
  //
  // Kept in localStorage rather than the database: it is a per-browser view
  // preference, it must survive a reload, and persisting it server-side would
  // quietly make it a second account setting to reconcile with My Alerts.
  const STORE_KEY = 'dxca.spotfilter';
  function restore(field: string): Set<string> {
    try {
      const raw = JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}');
      return new Set(Array.isArray(raw[field]) ? raw[field] : []);
    } catch {
      return new Set();
    }
  }
  let levelFilter = $state<Set<string>>(restore('levels'));
  let modeFilter = $state<Set<string>>(restore('modes'));
  let bandFilter = $state<Set<string>>(restore('bands'));

  $effect(() => {
    const payload = {
      levels: [...levelFilter],
      modes: [...modeFilter],
      bands: [...bandFilter],
    };
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(payload));
    } catch {
      // Private mode / storage disabled — the filter still works this session.
    }
  });

  onMount(() => {
    (async () => {
      await Promise.all([loadReference(), loadChase()]);
      const r = await api('GET', '/api/spots?limit=500');
      if (r.json?.spots) spots = r.json.spots;
      // Guarded, unlike the `r.json?.spots` above which is safe by its own
      // optional chaining. `api` no longer throws when the server is
      // unreachable — it returns status 0 — so an unguarded assignment here
      // would push an error object into the SHARED status store and the header
      // pill would report "0/0 nodes" on every screen. That states a fact
      // ("nothing is configured") in place of an admission ("I could not
      // ask"), which is the worse of the two by far.
      const s = await api('GET', '/api/status');
      if (s.status === 200) {
        status = s.json;
        setStatus(s.json);
      }
      const st = await api('GET', '/api/me/station');
      if (st.status === 200) station = st.json;
      const q = await api('GET', '/api/config/me/station');
      if (q.status === 200) locator = q.json?.locator ?? '';
      await loadSun();
    })();
    // The phase changes about four times a day, so a poll is enough — but
    // it must exist: a session left open across sunset would otherwise mask
    // the wrong bands for the whole evening.
    const sunTimer = setInterval(loadSun, 60_000);
    const stop = openStream((frame) => {
      if (frame.type === 'spot') {
        spots = [frame.spot, ...spots].slice(0, MAX_ROWS);
      } else if (frame.type === 'status') {
        status = frame.status;
        // The header pill reads the shared store, so hand it the live frame
        // rather than letting it poll a second, slower answer of its own.
        setStatus(frame.status);
      }
    });
    return () => {
      clearInterval(sunTimer);
      stop();
    };
  });

  async function loadSun() {
    const r = await api('GET', '/api/me/sun');
    // 204 = no locator, which is the ordinary state for most accounts.
    sun = r.status === 200 ? r.json : null;
  }

  const PHASE_LABEL: Record<string, string> = {
    dawn: 'Dawn',
    day: 'Day',
    dusk: 'Dusk',
    night: 'Night',
  };

  const hhmmUtc = (u: number | null | undefined) =>
    u == null ? '—' : `${hhmm(u)}Z`;

  const freqKHz = (s: any) =>
    (s.dial_frequency_hz + s.delta_frequency_hz) / 1000;

  // The award bucket the server would file this spot under. Mirrors
  // dxca-core's modes::canonical_opt so the display filter agrees with the
  // Telegram gate: anything unrecognised is DATA, but an EMPTY mode is null,
  // not DATA. Returning DATA there is what used to hide a phone spot behind
  // a digital filter, matching the server bug this pairs with.
  const PHONE = ['SSB', 'USB', 'LSB', 'AM', 'FM', 'PHONE', 'VOICE', 'DIGITALVOICE', 'C4FM', 'DMR', 'DSTAR'];
  function modeClass(mode: string | null | undefined): string | null {
    const m = (mode ?? '').trim().toUpperCase();
    if (!m) return null;
    if (m === 'CW') return 'CW';
    return PHONE.includes(m) ? 'PHONE' : 'DATA';
  }

  // Which set of award totals the card shows. The server sends both; the
  // tickbox chooses, so toggling costs no round trip.
  let shownStats = $derived(
    station ? pick(station.stats, station.stats_current) : null,
  );

  let sourceNames = $derived(
    Object.keys(status?.spots_per_source ?? {}).sort(),
  );

  let searchTerm = $derived(search.trim().toUpperCase());

  let visible = $derived.by(() => {
    let rows = spots.filter((s) => {
      // The server decides this now. Sniffing the message text matched
      // every cluster spot, because those messages are synthesised as
      // "CQ <call>" whatever the spot actually reported.
      if (searchTerm) {
        const hay = `${s.dx_call ?? ''} ${s.spotter ?? ''}`.toUpperCase();
        if (!hay.includes(searchTerm)) return false;
      }
      if (spotterKind === 'human' && s.is_skimmer) return false;
      if (spotterKind === 'skimmer' && !s.is_skimmer) return false;
      if (cqOnly && !s.is_cq) return false;
      if (sourceFilter.size && !sourceFilter.has(s.source_name)) return false;
      if (bandFilter.size && (!s.band || !bandFilter.has(s.band))) return false;
      // A spot whose mode is unknown matches no mode narrowing — the same
      // rule the server applies when it declines to guess a slot.
      if (modeFilter.size) {
        const cls = modeClass(s.mode);
        if (!cls || !modeFilter.has(cls)) return false;
      }
      // A level narrowing shows ONLY those levels — picking "New DXCC" means
      // the feed becomes a New-DXCC feed, not "everything, DXCC highlighted".
      if (levelFilter.size && !levelFilter.has(s.alert)) return false;
      return true;
    });
    if (hideDupes) {
      // First occurrence per CALL-BAND-MODE within 60 s (1.x displayedSpots).
      const lastSeen = new Map<string, number>();
      rows = rows
        .slice()
        .sort((a, b) => b.time_unix - a.time_unix)
        .filter((s) => {
          if (!s.dx_call) return true;
          const key = `${s.dx_call}-${s.band ?? ''}-${s.mode?.toUpperCase()}`;
          const prev = lastSeen.get(key);
          // Rows walk newest→oldest: keep the newest, drop older repeats.
          if (prev !== undefined && prev - s.time_unix < 60) return false;
          lastSeen.set(key, s.time_unix);
          return true;
        });
    }
    const dir = sortDesc ? -1 : 1;
    // Hide mode removes rows HERE, after every other narrowing, so the
    // count below still sees them. A mask that both removed rows and lost
    // count of them would be the silent-filter failure this feature exists
    // to avoid.
    if (bandMask.mode === 'hide') rows = rows.filter((s) => !hidden(s));
    return rows.slice().sort((a, b) => {
      let va = a[sortKey], vb = b[sortKey];
      if (sortKey === 'freq') { va = freqKHz(a); vb = freqKHz(b); }
      if (va == null) return 1;
      if (vb == null) return -1;
      return (va < vb ? -1 : va > vb ? 1 : 0) * dir;
    });
  });

  function sortBy(key: string) {
    if (sortKey === key) sortDesc = !sortDesc;
    else { sortKey = key; sortDesc = true; }
  }

  // The sort marker rides the active column only — a caret on every header
  // makes the row look like ten controls instead of one answer.
  const caret = (key: string) =>
    sortKey === key ? (sortDesc ? '↓' : '↑') : '';

  // Both the label and the colour now come from one place: the label from
  // the server's own AlertLevel::label() via /api/reference, the colour from
  // app.css's [data-level] table. Adding a ninth level needs no edit here.
  const flagged = (s: any) => s.alert && s.alert !== 'worked' && s.alert !== 'none';


  // The mask is NOT a narrowing — it never removes a row, so it plays no
  // part in `visible` and cannot empty the table. It only counts what it
  // has receded, which is what the badge beside the spot count reports.
  let maskedCount = $derived(
    bandMask.mode === 'hide'
      ? spots.filter(hidden).length
      : visible.filter(masked).length,
  );

  // Every narrowing the operator can be holding, including the older
  // source/CQ ones — the empty state has to account for all of them or it
  // will blame the wrong control.
  let narrowed = $derived(
    levelFilter.size > 0 ||
      modeFilter.size > 0 ||
      bandFilter.size > 0 ||
      sourceFilter.size > 0 ||
      cqOnly ||
      spotterKind !== 'all',
  );

  // What the collapsed rail's badge reports. Counted per CONTROL, not per
  // chip: "3" should mean three things are narrowing the feed, not that three
  // bands are ticked. `hideDupes` is deliberately absent — it is on by default
  // and collapses repeats of a spot rather than withholding one, so counting
  // it would leave the badge permanently lit and mean nothing.
  let activeFilters = $derived(
    (searchTerm ? 1 : 0) +
      (spotterKind !== 'all' ? 1 : 0) +
      (cqOnly ? 1 : 0) +
      (levelFilter.size ? 1 : 0) +
      (modeFilter.size ? 1 : 0) +
      (bandFilter.size ? 1 : 0) +
      (sourceFilter.size ? 1 : 0),
  );

  function clearFilters() {
    levelFilter = new Set();
    modeFilter = new Set();
    bandFilter = new Set();
    sourceFilter = new Set();
    cqOnly = false;
    spotterKind = 'all';
  }

  // Node state in the shared status-dot vocabulary: proven = up, connected
  // but nothing proven through it yet = amber, neither = down.
  const nodeDot = (n: any) =>
    n.proven ? 'on' : n.connected ? 'warn' : 'err';
</script>

<!-- Rail on the left, feed on the right. The five filter rows that used to
     stack above the table are the same controls in the same order — they have
     simply stopped competing with the feed for the one axis it needs. -->
<div class="feedpage">
  <FilterRail activeCount={activeFilters}>
    <div class="railgroup">
      <input
        class="search"
        type="search"
        placeholder="Call or spotter"
        bind:value={search}
        aria-label="Filter spots by callsign or spotter"
      />
      {#if searchTerm}
        <button class="clear" onclick={() => (search = '')}>Clear search</button>
      {/if}
    </div>

    <div class="railgroup">
      <span class="railhead">Show</span>
      <label class="flabel"><input type="checkbox" bind:checked={hideDupes} />Hide duplicates</label>
      <label class="flabel"><input type="checkbox" bind:checked={cqOnly} />CQ only</label>
      <!-- Who heard it. Beside Hide duplicates because both answer "which
           copies of this do I want to see", not "which spots interest me" —
           the chips below are for that. -->
      <div class="spotterkind">
        <span class="railhead">Spotted by</span>
        <div class="segmented" role="group" aria-label="Who made the spot">
          <button class:active={spotterKind === 'all'} onclick={() => (spotterKind = 'all')}
            title="Every spot, however it was heard.">All</button>
          <button class:active={spotterKind === 'human'} onclick={() => (spotterKind = 'human')}
            title="Only spots a person typed — skimmers (the -# callsigns) removed.">Human</button>
          <button class:active={spotterKind === 'skimmer'} onclick={() => (spotterKind = 'skimmer')}
            title="Only what the skimmers heard. A rare prefix usually shows up on a CW skimmer sweep before anyone types it.">Skimmer</button>
        </div>
      </div>
      <!-- Only offered once a locator exists, because without one the server
           sends no band advice and a permanently dead checkbox is worse than
           no checkbox. The route to it is the note beside the Locator field
           in Settings. -->
      {#if locator}
        <label
          class="flabel"
          title="Recede spots on bands the sun says are not plausibly workable from {locator} right now. New DXCC is never masked — see docs/PHASE-ROTATION-MASK.md."
          ><input type="checkbox" bind:checked={bandMask.on} />Band mask</label
        >
        <!-- Only offered once the mask is on: a mode selector for a switched
             off feature is a control that does nothing. Dim is the default and
             stays first — hide is the deliberate choice, not the obvious one. -->
        {#if bandMask.on}
          <div class="maskrow">
            <select
              class="maskmode"
              bind:value={bandMask.mode}
              aria-label="What the band mask does to masked spots"
              title="Dim keeps every spot on the page, receded, and restores it on hover — it cannot cost you a contact. Hide removes them from the list, which is cleaner on a busy feed."
            >
              <option value="dim">dim</option>
              <option value="hide">hide</option>
            </select>
            <!-- What the mask is reasoning from, shown rather than trusted. The
                 phase is the whole input to the model, and the two times are how
                 an operator judges whether the greyline window is set right. -->
            {#if sun}
              <span
                class="phase"
                data-phase={sun.phase}
                title="Sunrise {hhmmUtc(sun.sunrise_unix)}, sunset {hhmmUtc(
                  sun.sunset_unix,
                )} at {sun.locator}. Grey line is {sun.greyline_window_min} min either side — change it in Settings › My station."
                >{PHASE_LABEL[sun.phase] ?? sun.phase}</span
              >
            {/if}
          </div>
        {/if}
      {/if}
    </div>

    <ChipGroup
      stacked
      label="Sources"
      options={sourceNames.map((n) => ({ key: n, label: n }))}
      bind:selected={sourceFilter}
    />
    <!-- Only the levels this account can actually see: the classic eight
         plus chased awards (Settings › My station › Awards). -->
    <ChipGroup stacked label="Alerts" options={chasedLevels(levels())} bind:selected={levelFilter} levelKeys />
    <ChipGroup stacked label="Modes" options={modes()} bind:selected={modeFilter} />
    <ChipGroup stacked label="Bands" options={bands()} bind:selected={bandFilter} />
  </FilterRail>

  <div class="feedmain">
    <!-- The station card, flattened to a line. Whose log is driving the
         highlighting and how far along it is are still the first thing on the
         screen; they just no longer cost 190px to say. Worked sits beside
         confirmed because the gap between them IS what the ? levels close. -->
    {#if station}
      <div class="stationline">
        <span class="who mono">{station.log_callsign ?? station.callsign}</span>
        {#if station.display_name}<span class="opname">{station.display_name}</span>{/if}
        {#if station.log_callsign && station.log_callsign !== station.callsign}
          <span class="opname">log · signed in as {station.callsign}</span>
        {/if}
        {#if station.stats}
          <span class="award">DXCC <b>{shownStats.dxcc_worked}</b><span class="sep">/</span><span class="conf">{shownStats.dxcc_confirmed}</span></span>
          <span class="award">Challenge <b>{shownStats.challenge_worked}</b><span class="sep">/</span><span class="conf">{shownStats.challenge_confirmed}</span></span>
          <span class="award">Slots <b>{shownStats.slots_worked}</b><span class="sep">/</span><span class="conf">{shownStats.slots_confirmed}</span></span>
          {#if station.qso_count}
            <span class="award">QSOs <b>{station.qso_count}</b></span>
            <span class="opname">refreshed {ago(station.last_refresh_unix)} ago</span>
          {/if}
          <HelpTip label="Your totals">
            <span class="para">
              <b>Worked / confirmed</b> throughout — the gap between the two is
              the QSL chase the <b>?</b> levels exist to close.
            </span>
            <span class="para">
              <b>Challenge</b> is one point per entity per band over 160–6m (60m
              excluded, WARC included), mode-agnostic; 1000 confirmed points to
              claim. <b>Slots</b> are band × mode combinations — a different
              count, which is why the two are never added together.
            </span>
          </HelpTip>
          {#if canFilter(station.stats_current)}
            <label
              class="include-deleted"
              title="Totals count current DXCC entities by default, matching the ARRL standings. Tick to add the 62 deleted entities — Abu Ail, Blenheim Reef, British North Borneo and the rest. Those QSOs are in your log either way; they just score nothing."
            >
              <input type="checkbox" bind:checked={awards.includeDeleted} />incl. deleted
            </label>
          {/if}
        {:else}
          <span class="opname">
            No log loaded — set your ClubLog credentials in <b>Settings › My
            station</b> and refresh to get New/? highlighting.
          </span>
        {/if}
        <span class="counts">
        <span class="count muted">{visible.length} spots</span>
        <!-- Never silent: a mask that changes the screen without saying so is
             indistinguishable from a feed going quiet. Nothing is removed in
             dim mode, so the count says "dimmed", not "hidden". -->
        {#if bandMask.on && maskedCount > 0}
          <span
            class="count masked-count"
            title={bandMask.mode === 'hide'
              ? 'Removed from the list by the band mask. New DXCC is never hidden — switch to "dim" to see these again.'
              : 'Dimmed, not hidden — every one of them is still in the table and still sortable. New DXCC is never dimmed.'}
            >{maskedCount} {bandMask.mode === 'hide' ? 'hidden' : 'dimmed'}</span
          >
        {/if}
        </span>
      </div>
    {/if}

    <div class="card feed">
      <div class="table-wrap">
        <!-- FIXED widths, declared once here rather than left to the content.
             Auto layout sized each column to whatever happened to be in view,
             so a wide DXCC name arriving on the stream shifted every column
             right of it — the table re-flowed under the eye several times a
             second. Every width below is measured: the widest real value the
             column can hold (or its own header plus the sort caret, which is
             what sizes Time), at the feed's own 0.85rem system-ui, plus the
             0.9rem cell padding. DXCC is the exception — it is set to the
             narrowest width at which no two of the 340 current entity names
             clip to the same string, which is 11.5rem; 25 of them show an
             ellipsis and carry the full name on hover. -->
        <table>
          <colgroup>
            <col class="c-time" /><col class="c-call" /><col class="c-spot" />
            <col class="c-src" /><col class="c-freq" /><col class="c-mode" />
            <col class="c-db" /><col class="c-band" /><col class="c-dxcc" />
            <col class="c-al" /><col class="c-msg" />
          </colgroup>
          <thead>
            <tr>
              <th onclick={() => sortBy('time_unix')}>Time<i>{caret('time_unix')}</i></th>
              <!-- DX and DE, the operator's own words: the station being
                   spotted, and the station reporting it. "DX Call" and
                   "Spotter" said the same thing in twice the width, and the
                   two now sit side by side where the pair reads as a pair. -->
              <th onclick={() => sortBy('dx_call')} title="The station being spotted">DX<i>{caret('dx_call')}</i></th>
              <th onclick={() => sortBy('spotter')} title="The station that heard it">DE<i>{caret('spotter')}</i></th>
              <th onclick={() => sortBy('source_name')} title="The feed that carried the spot">Source<i>{caret('source_name')}</i></th>
              <th onclick={() => sortBy('freq')} title="Frequency in kHz">Freq<i>{caret('freq')}</i></th>
              <th onclick={() => sortBy('mode')}>Mode<i>{caret('mode')}</i></th>
              <th onclick={() => sortBy('snr_db')} title="Signal-to-noise, dB">dB<i>{caret('snr_db')}</i></th>
              <th onclick={() => sortBy('band')}>Band<i>{caret('band')}</i></th>
              <th onclick={() => sortBy('dxcc_name')}>DXCC<i>{caret('dxcc_name')}</i></th>
              <th onclick={() => sortBy('alert')}>Alert<i>{caret('alert')}</i></th>
              <th>Message</th>
            </tr>
          </thead>
          <tbody>
            {#each visible as s}
              <!-- A cluster spot's `message` is synthesised; `comment` is what
                   the spotter actually typed, so prefer it. Hoisted to the
                   top of the block because `{@const}` may only be an
                   immediate child of the `{#each}`. -->
              {@const msg = `${s.is_beacon ? '[BEACON] ' : ''}${s.comment || s.message}`}
              <tr
                class:flagged={flagged(s)}
                class:beacon={!flagged(s) && s.is_beacon}
                class:masked={masked(s)}
                data-level={flagged(s) ? s.alert : undefined}
                title={masked(s)
                  ? `${s.band} is not plausibly open from ${locator} at this hour — dimmed, not hidden`
                  : undefined}
              >
                <td class="mono">{hhmm(s.time_unix)}Z</td>
                <td class="mono call">
                  {s.dx_call ?? '—'}{#if s.is_lotw}<span class="lotw" title="LoTW user">●</span>{/if}
                </td>
                <td
                  class="mono spotter"
                  title={s.spotter
                    ? `Spotted by ${s.spotter}${s.is_skimmer ? ' (skimmer)' : ''}, relayed by ${s.source_name}`
                    : 'Decoded here'}
                >
                  {s.spotter ?? '—'}{#if s.is_skimmer}<span class="skim" title="Skimmer"
                    >#</span
                  >{/if}
                </td>
                <td title={s.source_name}>{s.source_name}</td>
                <td class="mono">{freqKHz(s).toFixed(1)}</td>
                <td class="mode">
                  {#if s.mode_inferred}
                    <span
                      class="inferred"
                      title="Guessed from the frequency — this spot's comment carried no mode"
                      >{s.mode}</span
                    >
                  {:else if s.mode}
                    {s.mode}
                  {:else}
                    <span class="unknown" title="No mode reported and none could be inferred">—</span>
                  {/if}
                </td>
                <td class="mono">{s.snr_db}</td>
                <td>{s.band ?? ''}</td>
                <!-- The 25 longest entity names clip here; the title is what
                     makes that safe, so it is unconditional rather than
                     computed — a cell that fits simply repeats itself. -->
                <td title={s.dxcc_name ?? ''}>{s.dxcc_name ?? ''}</td>
                <td class="alert">{flagged(s) ? levelLabel(s.alert) : ''}</td>
                <td class="muted msg" title={msg}>{msg}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <!-- A narrowed feed that shows nothing looks identical to a dead feed.
             Say which of the two it is, and name the way out. -->
        {#if visible.length === 0}
          <p class="empty hint">
            {#if spots.length === 0}
              No spots yet — waiting for the first one.
            {:else if narrowed}
              None of the {spots.length} spots held match this narrowing.
              <button class="link" onclick={clearFilters}>Show everything</button>
            {:else}
              Nothing to show.
            {/if}
          </p>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  /* Rail | feed. `minmax(0, 1fr)` on the second track, not `1fr`: a grid item's
     default `min-width: auto` is its CONTENT width, and the feed table is
     sixty rem of nowrap columns — without the zero minimum the track refuses
     to shrink and the whole page grows a horizontal scrollbar instead of the
     card. */
  .feedpage {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
  }

  /* The feed owns the window: the station line keeps its own air and the table
     card takes whatever height is left, so the newest spot is never below the
     fold. This carries the page gutter itself — `.page` would have put the
     padding outside the rail too, and the rail is meant to be flush. */
  .feedmain {
    min-width: 0;
    padding: 0.9rem 1.25rem 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  /* --- The rail's own furniture --- */
  .railgroup {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .railhead {
    font-size: 0.62rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.11em;
    color: var(--muted);
  }

  .maskrow {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding-left: 1.35rem;
  }

  /* Its own line under the tickboxes: three segments do not sit on a tick's
     baseline, and at 12rem the rail has no room beside one anyway. */
  .spotterkind {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.35rem;
  }

  .spotterkind .segmented {
    width: 100%;
  }

  .spotterkind .segmented button {
    flex: 1;
    padding: 0.15rem 0.2rem;
    font-size: 0.75rem;
  }

  /* --- The station line ---
     What was a 190px card. Every fact it carried is still here; they are on
     one line because they are all short, and none of them is worth a row of
     its own on the screen where height is the scarce thing. */
  .stationline {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0.25rem 1.1rem;
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .stationline .who {
    font-size: 1.05rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: CanvasText;
  }

  .opname {
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .award {
    font-variant-numeric: tabular-nums;
  }

  .award b {
    color: CanvasText;
    font-weight: 600;
  }

  .award .sep {
    color: var(--muted);
    margin: 0 0.1rem;
  }

  /* Confirmed is the number that counts for an award; worked beside it is the
     chase still open. */
  .award .conf {
    color: var(--ok);
  }

  /* Sits with the totals it changes — it is part of reading the line, not part
     of narrowing the feed, which is why it is here and not in the rail. */
  .include-deleted {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--muted);
    font-size: 0.72rem;
    white-space: nowrap;
    cursor: pointer;
  }

  .empty {
    margin: 0;
    padding: 1.5rem 1rem;
    text-align: center;
  }

  /* Reads as the sentence's own escape hatch, not as a third button idiom. */
  .empty .link {
    border: none;
    background: transparent;
    color: var(--accent);
    font: inherit;
    padding: 0 0.15rem;
    cursor: pointer;
  }

  .empty .link:hover {
    text-decoration: underline;
  }

  /* Inherits the card/field vocabulary rather than inventing a control:
     same border, radius and focus ring as the Settings inputs. */
  .search {
    font: inherit;
    font-size: 0.8rem;
    padding: 0.25rem 0.5rem;
    width: 100%;
    min-width: 0;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .search:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .clear {
    font: inherit;
    font-size: 0.75rem;
    padding: 0.2rem 0.45rem;
    color: var(--muted);
    background: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
  }

  /* A spotter is a callsign; give it the same weight as one, but muted —
     the DX call is what the eye should land on first. */
  .spotter {
    color: var(--muted);
  }

  /* The `-#` the parser strips, put back as a marker rather than in the
     callsign — the call stays readable and greppable, the machine-ness
     stays visible. */
  .skim {
    margin-left: 0.15rem;
    opacity: 0.65;
    font-weight: 600;
  }

  .flabel {
    color: var(--muted);
    font-size: 0.8rem;
    gap: 0.3rem;
  }

  .count {
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }

  /* The counts are the line's last fact, so they take the slack — the awards
     stay grouped on the left and the numbers sit at the right margin. They are
     wrapped rather than pushed individually because `:first-of-type` selects
     by TAG, and the first <span> on this line is the callsign. */
  .stationline .counts {
    margin-left: auto;
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    white-space: nowrap;
  }

  /* The card holds the scroll rather than the page, so the header rule and
     the card's own edge stay put while the feed moves under them. */
  .feed {
    padding: 0;
    min-height: 14rem;
    overflow: hidden;
  }

  /* Taller than it used to be, by exactly what the status boxes and the
     stacked filter rows gave back. */
  .table-wrap {
    overflow: auto;
    max-height: calc(100vh - 8.5rem);
  }

  /* --- The fixed grid ---
     `table-layout: fixed` is the whole point: it makes the browser take the
     widths from the colgroup and stop measuring content, so a column lands on
     the same x in every row and stays there while the stream runs. */
  table {
    table-layout: fixed;
  }

  /* Measured at 0.85rem system-ui, plus the 0.9rem cell padding, plus the
     0.75rem the sort caret reserves on every header. Time is set by its own
     header rather than by a timestamp; DXCC is set by the clip threshold (see
     the note in the markup); Message takes what is left. */
  col.c-time { width: 4.75rem; }
  col.c-call { width: 6.75rem; }
  col.c-spot { width: 6rem; }
  col.c-src  { width: 8rem; }
  col.c-freq { width: 5.75rem; }
  col.c-mode { width: 4.5rem; }
  col.c-db   { width: 3rem; }
  col.c-band { width: 4rem; }
  col.c-dxcc { width: 11.5rem; }
  col.c-al   { width: 5.75rem; }
  col.c-msg  { width: auto; }

  /* Every cell clips rather than widening its column — and every cell that can
     clip carries a `title`, so the full value is always one hover away. */
  td {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* --- Alignment ---
     Fixed columns leave slack, and left-aligning every value pinned each one
     to the far edge of its box: "SYRIA" sat at the left of an 11.5rem DXCC
     column with an inch of nothing before the next field, so the row read as
     scattered rather than as a row. Centred, each value sits in its own cell.

     The two NUMERIC columns are the exception and stay right-aligned: kHz and
     dB are read by comparing them down the column, and centring
     "7040.0" over "14090.7" puts the decimal points in different places,
     which is the one thing tabular figures exist to prevent.

     Free prose stays left — a centred paragraph has no edge to read from. */
  th,
  td {
    text-align: center;
  }

  /* kHz and dB. */
  th:nth-child(5), td:nth-child(5),
  th:nth-child(7), td:nth-child(7) {
    text-align: right;
  }

  /* Message. */
  th:nth-child(11), td:nth-child(11) {
    text-align: left;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    /* Opaque: rows scroll underneath it. `--card-bg` is a mix over Canvas,
       so it is a solid colour, not a wash. */
    background: var(--card-bg);
    cursor: pointer;
    user-select: none;
    padding-top: 0.6rem;
    padding-bottom: 0.4rem;
    overflow: hidden;
  }

  th:first-child,
  td:first-child {
    padding-left: 1rem;
  }

  th:last-child,
  td:last-child {
    padding-right: 1rem;
  }

  /* The sort marker: reserved width on every header so turning it on doesn't
     shove the row sideways. Counted into every column width above. */
  th i {
    display: inline-block;
    width: 0.75rem;
    margin-left: 0.2rem;
    font-style: normal;
    color: var(--accent);
  }

  /* Row hover. Deliberately the same specificity as the `tr.a-*` washes below
     and declared ahead of them, so source order hands an alert row its level
     colour: the tint is information, the hover only a pointer. */
  tr:hover td {
    background: color-mix(in srgb, CanvasText 6%, Canvas);
  }

  .call {
    font-weight: 600;
  }

  .mode {
    color: var(--muted);
  }

  /* An inferred mode is a guess the operator should be able to see is one —
     award slots rest on it. Dotted underline rather than a colour, so it
     does not compete with the alert-level tints in the same row. */
  .inferred {
    border-bottom: 1px dotted currentColor;
    cursor: help;
  }

  .unknown {
    opacity: 0.5;
  }

  .lotw {
    color: var(--ok);
    margin-left: 3px;
    font-size: 9px;
    vertical-align: super;
  }

  .alert {
    font-weight: 600;
  }

  /* Eight levels, two rules. `data-level` on the row resolves `--lvl` and
     `--lvl-bg` from app.css's level table, and the cell and the wash both
     read them — so they can never disagree, and a new level needs no rule
     here at all. */
  tr.flagged td { background: var(--lvl-bg); }
  tr.flagged .alert { color: var(--lvl); }
  tr.beacon td { color: var(--muted); }

  /* The band mask: DIM, NEVER HIDE (docs/PHASE-ROTATION-MASK.md).

     A receded row keeps its place, its sort position and its alert tint —
     it simply stops competing for attention. Opacity rather than a muted
     colour precisely because it fades the level tint too: a New Band flag
     on a dead band should look like a quiet flag, not a loud one.

     Hover brings it back to full. That is the safety valve made physical —
     a dimmed row is always one pointer away from being read, so the mask
     can never turn a workable spot into a puzzle. It is also why this
     stays declared last: source order hands it the final word over the
     .flagged and .beacon rules above. */
  tr.masked td { opacity: 0.45; }
  tr.masked:hover td { opacity: 1; }

  /* Compact, and sized to sit in the rail rather than dominate it — it is a
     modifier on a tickbox, not a control in its own right. */
  .maskmode {
    font-size: 0.8rem;
    padding: 0.1rem 0.2rem;
  }

  /* The phase badge states the model's one input. Deliberately NOT colour
     coded by phase: the alert levels already own colour in this row, and a
     yellow "Dusk" beside an orange New Slot would read as a fifth alert
     level. Muted ink and a border, like a status pill. */
  .phase {
    font-size: 0.75rem;
    padding: 0.05rem 0.4rem;
    border: 1px solid color-mix(in srgb, CanvasText 25%, Canvas);
    border-radius: 999px;
    color: var(--muted);
    white-space: nowrap;
  }

  /* Dawn and dusk are the grey line — the phase worth noticing, because it
     is when the low bands come alive. One weight of emphasis, no hue. */
  .phase[data-phase='dawn'],
  .phase[data-phase='dusk'] {
    color: var(--fg);
    border-color: color-mix(in srgb, CanvasText 45%, Canvas);
  }

  /* Sits beside the spot count, not in place of it — the operator needs both
     numbers to read the screen. */
  .masked-count {
    color: var(--muted);
    font-style: italic;
  }
</style>
