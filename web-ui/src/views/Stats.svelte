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
  function restoreSeg(): 'feed' | 'clublog' | 'awards' {
    try {
      const v = localStorage.getItem(SEG_KEY);
      return v === 'clublog' || v === 'awards' ? v : 'feed';
    } catch {
      return 'feed';
    }
  }
  let seg = $state<'feed' | 'clublog' | 'awards'>(restoreSeg());
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

  /// Has a log refresh built the mode axis yet? Three zeroes means "not
  /// rebuilt since 2.18.0", not "you have worked nothing" — and the
  /// difference is worth saying out loud rather than rendering as data.
  /// This calendar year's Marathon row, which is the only one that is
  /// still being played for.
  let thisYear = $derived(
    (station?.award_stats?.marathon ?? []).find(
      (m: any) => m.year === new Date().getUTCFullYear(),
    ),
  );

  /// The WAZ worklist: mixed first, then each mode class — the same shape
  /// the WAS list has, so the two cards read alike.
  let wazNeeded = $derived.by(() => {
    const a = station?.award_stats;
    if (!a) return [];
    const rows = a.waz_missing?.length ? [{ mode: 'Mixed', zones: a.waz_missing }] : [];
    return rows.concat(a.waz_needed_by_mode ?? []);
  });

  let wasModeData = $derived(
    (station?.award_stats?.was_by_mode ?? []).some((r: any) => r.confirmed > 0),
  );

  /// The Triple Play worklist inverted: which states each MODE still wants.
  /// The server sends it per state because that is the honest shape of the
  /// gap; this is the readable shape of the same thing.
  let tpNeeded = $derived.by(() => {
    const byMode = new Map<string, string[]>();
    for (const g of station?.award_stats?.triple_play_missing ?? []) {
      for (const m of g.needed) {
        if (!byMode.has(m)) byMode.set(m, []);
        byMode.get(m)!.push(g.state);
      }
    }
    // The classifier's own order, so this list reads like every other mode
    // list in the app rather than however the states happened to arrive.
    const ORDER = ['CW', 'PHONE', 'DATA'];
    const modes = [...byMode]
      .map(([mode, states]) => ({ mode, states }))
      .sort((a, b) => ORDER.indexOf(a.mode) - ORDER.indexOf(b.mode));
    // Mixed leads: it is the same question — which states do I still want —
    // asked without a mode, and it belongs beside its own endorsements
    // rather than stranded as a line in the summary card.
    const mixed = station?.award_stats?.was_missing ?? [];
    return (mixed.length ? [{ mode: 'Mixed', states: mixed }] : []).concat(modes);
  });
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
      <!-- Its own segment rather than a card under My ClubLog: the awards
           are their own errand, and WAS alone now carries a band table, a
           mode table and a Triple Play worklist. -->
      {#if chasedAny()}
        <button role="tab" aria-selected={seg === 'awards'} class:active={seg === 'awards'} onclick={() => (seg = 'awards')}
          >Awards</button
        >
      {/if}
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

  {#if seg === 'awards'}
    {#if station?.award_stats && chasedAny()}
      <div class="card">
        <h2>
          Awards
          <HelpTip label="Awards">
            <span class="para">
              Only the awards ticked under <b>Settings › My station ›
              Awards</b> appear here. Worked comes from your ClubLog log;
              <b>confirmed needs the login on LoTW account</b>, because
              ClubLog's export carries no state, island or QSL detail.
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
          {#if isChased('waz')}
            <div><dt>Zones worked</dt><dd class="num">{station.award_stats.waz_worked} / 40</dd></div>
            <div>
              <dt>Zones confirmed</dt>
              <dd class="num ok-num">{station.award_stats.waz_confirmed} / 40</dd>
            </div>
          {/if}
          {#if isChased('marathon') && thisYear}
            <div>
              <dt>Marathon {thisYear.year}</dt>
              <dd class="num ok-num">{thisYear.score}</dd>
            </div>
          {/if}
          {#if isChased('was')}
            <div><dt>States worked</dt><dd class="num">{station.award_stats.was_worked}</dd></div>
            <div>
              <dt>States confirmed</dt>
              <dd class="num ok-num">{station.award_stats.was_confirmed}</dd>
            </div>
            <div>
              <dt>Triple Play</dt>
              <dd class="num ok-num">{station.award_stats.triple_play} / 50</dd>
            </div>
          {/if}
        </dl>

      </div>

      {#if isChased('was')}
        <div class="card">
          <h2>
            WAS endorsements
            <HelpTip label="WAS endorsements">
              <span class="para">
                The same fifty states counted <b>per mode</b> and <b>per
                band</b> — the endorsements ARRL issues on top of basic WAS.
                <b>Triple Play</b> is all fifty in CW, Phone and Digital:
                150 confirmations, any band, LoTW only.
              </span>
              <span class="para">
                One number per slice, not worked-and-confirmed: a state only
                ever enters this table through your LoTW QSL report, so
                every one of them is confirmed by definition. There is no
                worked-but-unconfirmed state for DXCA to know about.
              </span>
            </HelpTip>
          </h2>

          {#if wasModeData}
            <h2>By mode</h2>
            <dl class="stats">
              {#each station.award_stats.was_by_mode as r (r.key)}
                <div><dt>{r.key}</dt><dd class="num">{r.confirmed} / 50</dd></div>
              {/each}
            </dl>

            {#if tpNeeded.length}
              <!-- BY MODE, not by state. Fifty state chips each listing three
                   modes is a wall you cannot read; three lines you can scan
                   for the mode you are actually operating is the same fact. -->
              <h2>Still needed</h2>
              <dl class="needlist">
                {#each tpNeeded as n (n.mode)}
                  <dt>{n.mode}</dt>
                  <dd class="mono">{n.states.join(' ')}</dd>
                {/each}
              </dl>
            {:else}
              <p class="hint">
                Nothing outstanding — all fifty states, mixed and in all
                three modes.
              </p>
            {/if}
          {:else}
            <!-- The mode axis arrived in 2.18.0 and is filled by a rebuild.
                 Saying so beats printing three zeroes and a fifty-state
                 worklist that only means "no data". -->
            <p class="hint">
              No mode data yet. The mode axis is built during a log refresh —
              run <b>Settings › My station › ClubLog › Refresh log now</b> once
              and Triple Play appears here.
            </p>
          {/if}

          {#if station.award_stats.was_by_band.length}
            <h2>By band</h2>
            <dl class="stats">
              {#each station.award_stats.was_by_band as r (r.key)}
                <div><dt>{r.key}</dt><dd class="num">{r.confirmed}</dd></div>
              {/each}
            </dl>
          {/if}
        </div>
      {/if}

      {#if isChased('waz')}
        <div class="card">
          <h2>
            WAZ
            <HelpTip label="WAZ">
              <span class="para">
                The forty CQ zones. Zones come from your log's own
                <code>CQZ</code> field, which ClubLog does export — so this
                award works without a LoTW report, unlike WAS and IOTA.
              </span>
              <span class="para">
                For <b>US</b> calls the zone is taken from the FCC state
                instead: cty.xml has no US call-area records and answers 5
                for the whole country, which would make zones 3 and 4
                unreachable.
              </span>
            </HelpTip>
          </h2>
          <!-- Worked AND confirmed, because for zones the two genuinely
               differ: they come from the ClubLog log, where a QSO can be
               made and never QSLed. (The WAS card shows one number for the
               opposite reason — a state only ever arrives already
               confirmed.) The gap between them is the chase. -->
          <h2>By mode <span class="subnote">worked / confirmed</span></h2>
          <dl class="stats">
            {#each station.award_stats.waz_by_mode as r (r.key)}
              <div>
                <dt>{r.key}</dt>
                <dd class="num">{r.worked} / <span class="ok-num">{r.confirmed}</span></dd>
              </div>
            {/each}
          </dl>

          <h2>Still needed</h2>
          {#if wazNeeded.length}
            <!-- Confirmed-wise, like the counts above it: an award is
                 claimed on confirmations, so a worked-but-unconfirmed zone
                 is still wanted. -->
            <dl class="needlist">
              {#each wazNeeded as n (n.mode)}
                <dt>{n.mode}</dt>
                <dd class="mono">{n.zones.join(' ')}</dd>
              {/each}
            </dl>
          {:else}
            <p class="hint">
              Nothing outstanding — all forty zones confirmed, mixed and in
              all three modes.
            </p>
          {/if}

          {#if station.award_stats.waz_by_band.length}
            <h2>By band <span class="subnote">worked / confirmed</span></h2>
            <dl class="stats">
              {#each station.award_stats.waz_by_band as r (r.key)}
                <div>
                  <dt>{r.key}</dt>
                  <dd class="num">{r.worked} / <span class="ok-num">{r.confirmed}</span></dd>
                </div>
              {/each}
            </dl>
          {/if}
        </div>
      {/if}

      {#if isChased('marathon') && station.award_stats.marathon.length}
        <div class="card">
          <h2>
            DX Marathon
            <HelpTip label="DX Marathon">
              Entities plus CQ zones worked in a calendar year — one point
              each, reset every January. No bands, no modes, no
              confirmation. The current year is the live score; the years
              below it are what the log remembers.
            </HelpTip>
          </h2>
          <table class="slices">
            <thead>
              <tr><th>Year</th><th>Entities</th><th>Zones</th><th>Score</th></tr>
            </thead>
            <tbody>
              {#each station.award_stats.marathon.slice(0, 8) as y (y.year)}
                <tr>
                  <td class="mono">{y.year}</td>
                  <td class="num">{y.entities}</td>
                  <td class="num">{y.zones}</td>
                  <td class="num ok-num">{y.score}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

      {#if isChased('vucc')}
        <div class="card">
          <h2>VUCC by band</h2>
          {#if station.award_stats.vucc.length}
            <table class="slices">
              <thead><tr><th>Band</th><th>Grids worked</th><th>Confirmed</th></tr></thead>
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
          {:else}
            <p class="hint">No 50 MHz+ grids in the log yet — VUCC counts from 6M up.</p>
          {/if}
        </div>
      {/if}
    {:else}
      <div class="card">
        <p class="hint">
          No awards ticked yet — pick them under
          <b>Settings › My station › Awards</b>.
        </p>
      </div>
    {/if}
  {:else if seg === 'feed'}
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

  /* --- One scale, not one per section ---------------------------------
     Counts sat at 1.05rem, the mono lists at the body's 1rem and table
     cells at whatever they inherited, so reading down the tab each block
     looked like a different document. Everything that is DATA now takes
     `--fs-item`, the size app.css already defines for exactly that, and
     every section heading is the plain `<h2>` the rest of the app uses
     for a second heading inside a card. No new sizes: the stylesheet
     declares four roles and says a screen must not invent a fifth. */
  .stats dd {
    margin: 0.1rem 0 0;
    font-size: var(--fs-item);
  }

  .stats dd.num {
    font-variant-numeric: tabular-nums;
  }

  /* One row per mode: the label in the gutter, the states wrapping beside
     it. Three readable lines instead of fifty chips. */
  /* `max-content` on the gutter keeps the three labels aligned without
     stretching, and dt/dd are the grid's OWN children — wrapping them in a
     div made each mode a single cell, so two landed per row and the third
     was orphaned underneath. */
  .needlist {
    margin: 0;
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.3rem 0.9rem;
    align-items: baseline;
  }

  .needlist dt {
    color: var(--muted);
    font-size: var(--fs-hint);
  }

  .needlist dd {
    margin: 0;
    font-size: var(--fs-item);
    line-height: 1.6;
    word-spacing: 0.15em;
  }

  .slices {
    width: auto;
    font-size: var(--fs-item);
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

  p {
    margin: 0.75rem 0 0;
  }
</style>
