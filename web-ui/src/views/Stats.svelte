<script lang="ts">
  // Stats: two segments, two different questions.
  //
  // **Feed** — what the spot ring is made of right now. **ClubLog** — what your
  // log holds. They are under one tab because "how am I doing" is one errand,
  // and behind a segmented control rather than stacked because they count
  // completely different things and must not be mistaken for one dataset: one
  // counts spots in a rolling window, the other counts DXCC entities since
  // 1945.
  //
  // The ClubLog half used to be the bottom of the My ClubLog tab, under the
  // same Save button as the credentials that fetch it. Those went to Settings;
  // this is the reading that was left.
  //
  // **Bars, not pie charts.** The job on the feed side is magnitude comparison
  // — "which band carries most of my spots" — across up to fifteen categories
  // with long names like "UberSDR CWskim". A pie with fifteen slices is
  // unreadable and cannot hold its own labels; horizontal bars compare exactly,
  // sort meaningfully, and leave room for the text.
  //
  // Each chart is a SINGLE series, so there is no legend: the heading names it,
  // and colour carries no meaning beyond "this is the bar".
  import { api, ago } from '../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { awards, pick, canFilter } from '../lib/awards.svelte';
  import { loadChase, chasedAny, isChased } from '../lib/chase.svelte';
  import { currentTheme } from '../lib/theme.svelte';

  // ClubLog's DX Dash picks its own appearance — a `dxd-theme` cookie if the
  // browser sends one, otherwise BY THE CLOCK: light 07:00–19:59 on the
  // browser's clock, dark at night (the inline script in its <head>, read
  // 2026-09-01). Neither input is ours to set: inside this iframe the cookie
  // is third-party (Safari refuses those outright, SameSite=Lax keeps them
  // out elsewhere), and there is no URL parameter. So left alone the embed
  // turns dark at 8 pm however the rest of this screen is set.
  //
  // The clock rule reads the browser's clock — the same clock this page has
  // — so the frame's choice is exactly predictable. Captured when the frame
  // LOADS (it is lazy, so mount time is not load time), and when its
  // appearance disagrees with the app's, a CSS invert+hue-rotate flips it
  // back: lightness swaps, hues stay put, so the map and charts keep their
  // own colours. If ClubLog ever honours prefers-color-scheme or grows a
  // theme parameter, delete all of this and use that instead.
  let embedTheme = $state<'light' | 'dark'>('light');
  function embedLoaded() {
    const h = new Date().getHours();
    embedTheme = h >= 7 && h < 20 ? 'light' : 'dark';
  }

  // Which half is showing. Persisted per browser, because the segmented
  // control turned out to be easy to miss entirely: the page lands on the feed
  // charts, and an operator who came looking for their log statistics saw
  // three bar charts and concluded the table had been lost in the rework.
  // Remembering the choice means it has to be found once, not every visit.
  const SEG_KEY = 'dxca.statsseg';
  function restoreSeg(): 'feed' | 'clublog' {
    try {
      return localStorage.getItem(SEG_KEY) === 'clublog' ? 'clublog' : 'feed';
    } catch {
      return 'feed';
    }
  }
  let seg = $state<'feed' | 'clublog'>(restoreSeg());
  $effect(() => {
    try {
      localStorage.setItem(SEG_KEY, seg);
    } catch {
      // Private mode / storage disabled — the switch still works this session.
    }
  });

  // --- Feed ------------------------------------------------------------------
  let stats = $state<any>(null);
  let error = $state('');

  async function loadFeed() {
    const r = await api('GET', '/api/spot-stats');
    if (r.status === 200) {
      stats = r.json;
      error = '';
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  onMount(() => {
    loadFeed();
    loadStation();
    loadChase();
    // The ring turns over in under an hour on a busy feed, so a static
    // snapshot goes stale while you look at it.
    const t = setInterval(loadFeed, 15000);
    return () => clearInterval(t);
  });

  /// Longest bar in a group defines full width, so each chart uses its own
  /// scale. Comparing across the three would be meaningless — they count
  /// the same spots three different ways.
  const peak = (rows: any[]) => Math.max(1, ...rows.map((r) => r.count));

  const pct = (n: number, total: number) =>
    total > 0 ? ((n / total) * 100).toFixed(1) : '0.0';

  /// Returns the whole phrase, "about" included, because the short case
  /// cannot take that hedge: a freshly started instance reading "about 0 min
  /// of feed" states a measurement where there isn't one yet, and "about
  /// under a minute" is not English.
  function span(secs: number): string {
    if (!secs) return '';
    const m = Math.round(secs / 60);
    if (m < 1) return 'under a minute';
    if (m < 90) return `about ${m} min`;
    return `about ${(m / 60).toFixed(1)} hours`;
  }

  // --- ClubLog ---------------------------------------------------------------
  // The same endpoint the Spots station line uses, so the two can never
  // disagree about what the log holds.
  let station = $state<any>(null);
  async function loadStation() {
    const r = await api('GET', '/api/me/station');
    if (r.status === 200) station = r.json;
  }
  // Both screens read the same shared preference, so the totals here and the
  // station line on Spots can never disagree about which entities count.
  let shownStats = $derived(station ? pick(station.stats, station.stats_current) : null);
  let shownBandMode = $derived(
    station ? pick(station.by_band_mode, station.by_band_mode_current) : null,
  );

  /// The grid's "Total" column for one mode row — that mode's entity count
  /// across every band. Deliberately read from `modes` rather than summed
  /// from the row: an entity worked on 20M and 40M in CW fills two cells but
  /// is one CW entity, so the row's sum would overcount it.
  function modeTotal(mode: string) {
    return shownBandMode?.modes.find((m) => m.key === mode) ?? { worked: 0, confirmed: 0 };
  }

  /// The callsign to embed: **the one set in Settings › My station › ClubLog
  /// account**, which the server returns as `log_callsign`. It can differ
  /// from the login — a /P, or a club log — and the log is what this card is
  /// about. Lowercased, the form clublog.org's own URLs use.
  ///
  /// Deliberately NO fallback to the login callsign. It would be unreachable
  /// today (the embed sits inside a branch that needs `station.stats`, and
  /// that is null until a log has been downloaded, which requires this
  /// callsign) — but the failure it would cause is bad enough to be worth
  /// closing anyway: falling back would frame SOMEONE ELSE's public dashboard
  /// under a heading that says "My ClubLog". Better to show nothing.
  let logCall = $derived((station?.log_callsign ?? '').trim().toLowerCase());
</script>

<div class="page statspage">
  <div class="segrow">
    <!-- Labelled, because unlabelled it read as decoration rather than as the
         switch between two different datasets. -->
    <span class="seglabel">Statistics for</span>
    <div class="segmented" role="tablist" aria-label="Which statistics">
      <button role="tab" aria-selected={seg === 'feed'} class:active={seg === 'feed'} onclick={() => (seg = 'feed')}
        >The spot feed</button
      >
      <button role="tab" aria-selected={seg === 'clublog'} class:active={seg === 'clublog'} onclick={() => (seg = 'clublog')}
        >My ClubLog</button
      >
    </div>
    <HelpTip label="Two kinds of statistic">
      <span class="para">
        <b>Feed</b> counts the spots DXCA is holding in memory right now — a
        rolling window, shared by every account on this server.
      </span>
      <span class="para">
        <b>ClubLog</b> counts DXCC entities in <em>your</em> log since 1945.
        Different units, different scope: they are never added together, which
        is why they sit behind a switch rather than on one page.
      </span>
    </HelpTip>
  </div>

  {#if seg === 'feed'}
    {#if error}
      <div class="card"><p class="err">{error}</p></div>
    {:else if !stats}
      <div class="card"><p class="hint">Counting…</p></div>
    {:else}
      <!-- The headline is a number, not a chart: one value has no shape to
           plot, and a gauge or a single-slice pie would be decoration. -->
      <div class="card total-card">
        <div class="total">
          <span class="num">{stats.total.toLocaleString()}</span>
          <span class="cap">
            spots held
            <HelpTip label="Spots held">
              Everything DXCA currently has in memory. The ring keeps the most
              recent spots and discards the oldest, so this is a window, not a
              running total since startup.
            </HelpTip>
          </span>
        </div>
        <!-- The one thing here that is a READING rather than an explanation —
             how much feed the ring is currently holding. On a freshly started
             instance there is no span yet and the line is absent entirely,
             rather than claiming "about 0 min of feed". -->
        {#if stats.span_secs}
          <p class="hint">{span(stats.span_secs)} of feed</p>
        {/if}
      </div>

      {#each [{ title: 'By band', hue: 'band', rows: stats.bands, note: 'In band order, not by count — this reads as a band plan.' }, { title: 'By mode', hue: 'mode', rows: stats.modes, note: 'As reported by the decoder or the spot comment, so FT8 and FT4 stay apart.' }, { title: 'By source', hue: 'source', rows: stats.sources, note: 'The feed that carried the spot — a decoder here, or the cluster node that relayed it.' }] as group (group.title)}
        <div class="card" data-hue={group.hue}>
          <h2>
            {group.title}
            <HelpTip label={group.title}>{group.note}</HelpTip>
          </h2>
          {#if !group.rows.length}
            <p class="hint">Nothing yet.</p>
          {:else}
            <div class="bars">
              {#each group.rows as row (row.key)}
                <div class="row">
                  <span
                    class="key"
                    title="{row.key}: {row.count} spots ({pct(row.count, stats.total)}% of the ring)"
                    >{row.key}</span
                  >
                  <span class="track">
                    <span class="bar" style="width: {(row.count / peak(group.rows)) * 100}%"></span>
                  </span>
                  <span class="val mono">{row.count.toLocaleString()}</span>
                  <span class="share mono">{pct(row.count, stats.total)}%</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  {:else if station?.stats}
    <!-- Every number is entities, not QSOs, because that is what the awards
         count — and worked sits beside confirmed throughout, since the gap
         between them IS the QSL chase. -->
    <div class="card">
      <h2>Log statistics</h2>
      <p class="hint sub">
        {station.qso_count ?? 0} QSOs
        {#if station.log_callsign}for <b>{station.log_callsign}</b>{/if}
        {#if station.last_refresh_unix}· refreshed {ago(station.last_refresh_unix)} ago{/if}
      </p>

      {#if canFilter(station.stats_current)}
        <label
          class="include-deleted"
          title="Totals count current DXCC entities by default, matching the ARRL standings. Tick to add the 62 deleted entities — Abu Ail, Blenheim Reef, British North Borneo and the rest. Those QSOs are in your log either way; they just score nothing."
        >
          <input type="checkbox" bind:checked={awards.includeDeleted} />include deleted entities
        </label>
      {/if}

      <dl class="stats">
        <div><dt>DXCC worked</dt><dd class="num">{shownStats.dxcc_worked}</dd></div>
        <div><dt>DXCC confirmed</dt><dd class="num ok-num">{shownStats.dxcc_confirmed}</dd></div>
        <div><dt>Challenge worked</dt><dd class="num">{shownStats.challenge_worked}</dd></div>
        <div><dt>Challenge confirmed</dt><dd class="num ok-num">{shownStats.challenge_confirmed}</dd></div>
        <div><dt>Slots worked</dt><dd class="num">{shownStats.slots_worked}</dd></div>
        <div><dt>Slots confirmed</dt><dd class="num ok-num">{shownStats.slots_confirmed}</dd></div>
      </dl>
    </div>

    <!-- Only the awards this account chases (Settings › My station ›
         Awards) — nothing here for anyone who has not opted in. -->
    {#if station.award_stats && chasedAny()}
      <div class="card">
        <h2>
          Awards
          <HelpTip label="Awards">
            <span class="para">
              The awards you chase under <b>Settings › My station › Awards</b>
              — only those show here. Worked comes from your ClubLog log;
              <b>confirmed needs the LoTW credentials</b> on the ClubLog
              account page, because ClubLog's export carries no state, island
              or QSL detail.
            </span>
            <span class="para">
              <b>VUCC</b> counts 4-character grid squares per band, 50 MHz
              and up only. <b>WAS</b> counts the fifty states, any band or
              mode; DC counts as Maryland. <b>IOTA</b> counts island groups.
            </span>
          </HelpTip>
        </h2>
        <dl class="stats">
          {#if isChased('iota')}
            <div><dt>IOTA worked</dt><dd class="num">{station.award_stats.iota_worked}</dd></div>
            <div>
              <dt>IOTA confirmed</dt>
              <dd class="num ok-num">{station.award_stats.iota_confirmed}</dd>
            </div>
          {/if}
          {#if isChased('was')}
            <div><dt>States worked</dt><dd class="num">{station.award_stats.was_worked}</dd></div>
            <div>
              <dt>States confirmed</dt>
              <dd class="num ok-num">{station.award_stats.was_confirmed}</dd>
            </div>
          {/if}
        </dl>
        {#if isChased('was') && station.award_stats.was_missing.length && station.award_stats.was_missing.length < 50}
          <!-- The chase list — fifty minus worked is short for anyone actually
               after WAS, and "which ones" is the question the number raises. -->
          <p class="hint">
            Missing states:
            <span class="mono">{station.award_stats.was_missing.join(' ')}</span>
          </p>
        {/if}
        {#if isChased('vucc') && station.award_stats.vucc.length}
          <!-- Same table dress as the feed slices — three narrow columns,
               numbers right-aligned, no new CSS to drift. -->
          <table class="slices">
            <thead>
              <tr><th>VUCC band</th><th>Grids worked</th><th>Confirmed</th></tr>
            </thead>
            <tbody>
              {#each station.award_stats.vucc as v (v.band)}
                <tr>
                  <td>{v.band}</td>
                  <td class="num">{v.worked}</td>
                  <td class="num ok-num">{v.confirmed}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else if isChased('vucc')}
          <p class="hint">No 50 MHz+ grids in the log yet — VUCC counts from 6M up.</p>
        {/if}
      </div>
    {/if}

    {#if shownBandMode}
      <div class="card">
        <h2>
          Entities per band and mode
          <HelpTip label="Entities per band and mode">
            Counts are <b>entities</b>, not QSOs: a cell is how many DXCC
            entities you have at least one contact with on that band, in that
            mode. Zeros are left visible — an empty cell is the most useful
            one here. Digital modes share one DATA bucket, matching the DXCC
            award rules.
            <br /><br />
            <b>Mixed is not the column added up.</b> An entity worked on 20M
            in both CW and DATA fills two cells but is still one entity on
            20M. Mixed counts entities per band whatever the mode; Total
            counts them per mode whatever the band.
          </HelpTip>
        </h2>
        <!-- Seventeen columns; it scrolls inside the card rather than
             widening the page, the same rule the Settings editors follow. -->
        <div class="editor-scroll">
          <table class="slices">
            <thead>
              <tr>
                <th>Band</th>
                <th class="num">Total</th>
                {#each shownBandMode.bands as b (b.key)}<th class="num">{b.key}</th>{/each}
              </tr>
            </thead>
            <tbody>
              {#each shownBandMode.grid as row (row.mode)}
                <tr>
                  <td>{row.mode} worked</td>
                  <td class="num" class:zero={!modeTotal(row.mode).worked}
                    >{modeTotal(row.mode).worked}</td
                  >
                  {#each row.bands as b (b.key)}
                    <td class="num" class:zero={!b.worked}>{b.worked}</td>
                  {/each}
                </tr>
                <tr>
                  <td>{row.mode} confirmed</td>
                  <td class="num ok-num" class:zero={!modeTotal(row.mode).confirmed}
                    >{modeTotal(row.mode).confirmed}</td
                  >
                  {#each row.bands as b (b.key)}
                    <td class="num ok-num" class:zero={!b.confirmed}>{b.confirmed}</td>
                  {/each}
                </tr>
              {/each}
              <tr class="mixed">
                <td>Mixed worked</td>
                <td class="num" class:zero={!shownStats.dxcc_worked}>{shownStats.dxcc_worked}</td>
                {#each shownBandMode.bands as b (b.key)}
                  <td class="num" class:zero={!b.worked}>{b.worked}</td>
                {/each}
              </tr>
              <tr class="mixed">
                <td>Mixed confirmed</td>
                <td class="num ok-num" class:zero={!shownStats.dxcc_confirmed}
                  >{shownStats.dxcc_confirmed}</td
                >
                {#each shownBandMode.bands as b (b.key)}
                  <td class="num ok-num" class:zero={!b.confirmed}>{b.confirmed}</td>
                {/each}
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    {/if}

    <!-- ClubLog's own view of the same log, embedded exactly as vu2cpl.com
         embeds it. It comes AFTER our own tables on purpose: the numbers
         above are what the alert levels are actually computed from, and are
         the ones the Spots station line must agree with. This is the same log
         drawn by someone else — a cross-check and a map, not the source of
         truth.

         Nothing is requested until this segment is opened, because the
         segment is `{#if}`-gated rather than merely hidden: an operator who
         never looks at it never makes a third-party request. -->
    {#if logCall}
      <div class="card">
        <h2>
          ClubLog DX Dashboard
          <HelpTip label="ClubLog DX Dashboard">
            <span class="para">
              Live from <b>clublog.org</b>, for <b>{logCall.toUpperCase()}</b> —
              their own charts and map for the log DXCA downloaded.
            </span>
            <span class="para">
              It is a page from ClubLog, not part of DXCA: your browser fetches
              it directly, and it needs the internet even though the rest of
              this screen does not.
            </span>
            <span class="para">
              Left alone it picks light or dark <b>by the clock</b> — light by
              day, dark after 8 pm — regardless of your appearance here. DXCA
              re-tints it to match this app, so the page never changes colour
              halfway down.
            </span>
          </HelpTip>
        </h2>
        <div class="embed">
          <div class="embed-head">
            <span class="embed-src mono">clublog.org/dx-dash/{logCall}</span>
            <a
              class="embed-out"
              href="https://clublog.org/logsearch/{logCall.toUpperCase()}"
              target="_blank"
              rel="noopener noreferrer">Full log search ↗</a
            >
          </div>
          <iframe
            src="https://clublog.org/dx-dash/{logCall}"
            title="{logCall.toUpperCase()} DX Dashboard, from ClubLog"
            height="1075"
            scrolling="no"
            loading="lazy"
            onload={embedLoaded}
            class:flip={currentTheme() !== embedTheme}
          ></iframe>
        </div>
      </div>
    {/if}
  {:else}
    <div class="card">
      <h2>Log statistics</h2>
      <p class="hint">
        No log loaded yet. Set your ClubLog credentials in
        <b>Settings › My station › ClubLog account</b> and press
        <b>Refresh log now</b> — the statistics appear here once it downloads.
      </p>
    </div>
  {/if}
</div>

<style>
  /* Fills the window, like Spots and Alerts do. It was `.narrow` (56rem),
     which is the right cap for a column of settings fields but wrong for
     this screen: the bar charts want every pixel of track they can get —
     that length IS the comparison — and the embedded ClubLog dashboard is a
     whole page in its own right that was being squeezed into two-thirds of
     the window while the rest sat empty. */
  .statspage {
    display: flex;
    flex-direction: column;
  }

  /* Prose still stops at a readable measure. Widening the SCREEN is not a
     reason to run a sentence across 1400px. */
  .statspage :global(.hint),
  .statspage :global(.help-pop-body) {
    max-width: 44rem;
  }

  .segrow {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-bottom: 1.25rem;
  }

  .seglabel {
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .total-card {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .total {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  /* The hero number carries the weight; its caption stays recessive so the
     eye lands on the value first. */
  .total .num {
    font-size: 2.4rem;
    font-weight: 700;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }

  .total .cap {
    color: var(--muted);
    font-size: 0.85rem;
  }

  /* One grid for the whole chart rather than one per row, and a FIXED label
     column so all three charts share it.

     `max-content` sized each chart to its own longest name, which read fine
     one chart at a time and ragged down the page: "160M" and "FT8" and
     "DB0SUE" each started their bars at a different x, so the three charts
     never lined up. Looking at the whole page is what showed it — the
     charts are stacked, so they are compared whether or not they were
     meant to be.

     The width is the answer to "what is the longest name here", and the
     labels WRAP rather than truncate when something exceeds it. Truncating
     a node's name in a chart about which node carried what would be exactly
     the wrong thing to lose; a two-line label costs one row of height and
     keeps every character. */
  .bars {
    display: grid;
    grid-template-columns: 7.5rem 1fr max-content max-content;
    align-items: center;
    /* 2px between fills, so adjacent bars read as separate marks rather
       than one block. */
    row-gap: 2px;
    column-gap: 0.6rem;
    font-size: 0.82rem;
  }

  .row {
    display: contents;
  }

  /* Wraps rather than overflows or truncates — see .bars above. */
  .key {
    color: var(--fg);
    overflow-wrap: anywhere;
  }

  /* The track is the recessive frame the bar sits in — it shows the scale
     without competing with the data. */
  .track {
    background: color-mix(in srgb, CanvasText 7%, Canvas);
    border-radius: 3px;
    height: 14px;
    overflow: hidden;
  }

  /* One hue per CHART, not per bar.

     Colour here answers "which breakdown am I reading", which is real if
     modest information; fifteen hues for fifteen bands would encode
     identity the labels already carry, and would fail on colour-vision
     grounds for nothing in return.

     None of these three is a colour DXCA already means something by. Red,
     orange and yellow are the DXCC/Slot/Mode alert levels, blue is New
     Band and the app accent, and green is the `ok` status — an orange bar
     in here would read as "New Slot". Teal, violet and pink are what is
     left, and each was run through the palette validator rather than
     picked by eye. */
  [data-hue='band'] {
    --series: light-dark(#0891b2, #22a7b3);
  }

  [data-hue='mode'] {
    --series: light-dark(#6639ba, #a371f7);
  }

  [data-hue='source'] {
    --series: light-dark(#bf3989, #db61a2);
  }

  /* Thin mark, rounded at the data end only: the baseline end stays square
     because it is anchored to zero, not a value. */
  .bar {
    display: block;
    height: 100%;
    background: var(--series, var(--accent));
    border-radius: 0 4px 4px 0;
    min-width: 2px;
  }

  /* The heading wears its chart's hue as a small marker, so the colour is
     tied to a name rather than floating free. The heading TEXT stays in
     the ordinary ink — text never wears the series colour. */
  .card[data-hue] h2::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 0.45rem;
    border-radius: 2px;
    background: var(--series, var(--accent));
    vertical-align: middle;
  }

  /* Values wear text tokens, never the series colour — identity is carried
     by the label beside them. */
  .val {
    text-align: right;
    color: var(--fg);
    font-variant-numeric: tabular-nums;
  }

  .share {
    text-align: right;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  /* --- The ClubLog segment --- */
  .include-deleted {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin: -0.2rem 0 0.7rem;
    color: var(--muted);
    font-size: 0.78rem;
    cursor: pointer;
  }

  .sub-head {
    margin: 1.25rem 0 0.5rem;
    font-size: 0.95rem;
  }

  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 0.9rem 2rem;
    margin: 0;
  }

  .stats dt {
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .stats dd {
    margin: 0.1rem 0 0;
    font-size: 1.05rem;
  }

  .stats dd.num {
    font-variant-numeric: tabular-nums;
  }

  .slices {
    width: auto;
    font-variant-numeric: tabular-nums;
  }

  .slices th,
  .slices td {
    padding: 0.2rem 0.75rem 0.2rem 0;
    white-space: nowrap;
  }

  .slices .num {
    text-align: right;
  }

  /* Confirmed is the number that counts for an award, so it reads as the
     positive one; worked beside it is the chase still open. */
  .ok-num {
    color: var(--accent);
  }

  /* A zero is deliberately quiet but present: an empty band is information,
     and blanking it would hide the gap worth working. */
  .zero {
    opacity: 0.35;
  }

  /* Mixed is the mode-agnostic summary of the rows above it, not another
     mode, so it gets a rule above it and a little more weight — the same
     separation RUMlog draws. */
  .slices tr.mixed td {
    border-top: 1px solid var(--border);
    font-weight: 600;
  }

  /* --- The ClubLog embed ---
     Framed and captioned so it reads as somebody else's page shown inside
     ours, rather than as part of DXCA. It wears ClubLog's own styling, which
     is theirs to choose — they run their own light/dark switch off the hour
     of day and a `dxd-theme` cookie, so it will not always agree with this
     app's appearance. Filtering it to match would misrepresent what is being
     shown.

     The attributes on the iframe deliberately MATCH vu2cpl.com's working
     embed rather than improving on it — same fixed height, same
     `scrolling="no"`. A `referrerpolicy="no-referrer"` was tried and dropped:
     the page sets its own `<meta name="referrer" content="same-origin">`, and
     guessing at a privacy tightening that the proven configuration does not
     have is not worth risking a blank frame nobody can debug from here. */
  .embed {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    /* The app's own ground, not #fff: the frame is blank until it lazy-loads
       (and flipped afterwards when the clock disagrees — see the script), so
       a hard white here would flash-bang a dark screen. */
    background: Canvas;
  }

  .embed-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.4rem 0.7rem;
    background: var(--card-bg);
    border-bottom: 1px solid var(--border);
  }

  /* Names the origin, because an embedded third-party page that does not say
     where it came from is the kind of thing an operator should be suspicious
     of in their own shack software. */
  .embed-src {
    font-size: 0.72rem;
    color: var(--muted);
  }

  .embed-out {
    font-size: 0.72rem;
    color: var(--accent);
    text-decoration: none;
    white-space: nowrap;
  }

  .embed-out:hover {
    text-decoration: underline;
  }

  /* Fixed height: the frame is cross-origin, so its content cannot report how
     tall it is and nothing can size it automatically. 1075px is what fits the
     dashboard whole — the same figure vu2cpl.com settled on. If ClubLog
     changes the dashboard's length this is the number to revisit; there is no
     way to make it adapt. */
  .embed iframe {
    display: block;
    width: 100%;
    border: none;
  }

  /* The counter-flip (see the script's note on ClubLog's clock rule):
     applied only when the frame's own pick disagrees with the app's, so a
     matching frame is never touched. */
  .embed iframe.flip {
    filter: invert(1) hue-rotate(180deg);
  }

  /* Sits between the card title and what it describes, so it takes the gap
     rather than adding one. */
  .sub {
    margin: -0.35rem 0 0.7rem;
    line-height: 1.45;
    max-width: 34rem;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
