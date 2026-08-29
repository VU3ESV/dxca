<script lang="ts">
  // The spots dashboard: status pills + live table (plan §8 page 2).
  // Backfills over /api/spots, then rides /api/stream; filters and the
  // 60 s duplicate collapse mirror the 1.x display behaviour.
  import { api, openStream, hhmm, ago } from '../lib/api';
  import { onMount } from 'svelte';
  import ChipGroup from '../lib/ChipGroup.svelte';
  import { awards, pick, canFilter } from '../lib/awards.svelte';
  import { bandMask, masked } from '../lib/bandmask.svelte';
  import { loadReference, bands, modes, levels, levelLabel } from '../lib/reference.svelte';

  let spots = $state<any[]>([]);
  let status = $state<any>(null);
  let station = $state<any>(null);
  /// The operator's Maidenhead square, or '' — the band mask's precondition.
  /// Asked for directly rather than inferred from whether spots carry
  /// `band_open`, because an empty feed or a batch of unclassified spots
  /// would make a configured locator look absent and hide the control.
  let locator = $state('');
  let sortKey = $state('time_unix');
  let sortDesc = $state(true);
  /// Free-text narrowing, matched against the spotted call and the spotter.
  /// Deliberately not persisted: a forgotten search that survives a reload
  /// looks exactly like a broken feed.
  let search = $state('');
  /// Hide skimmer spots. A skimmer's `-#` marker is stripped off the
  /// callsign, so without the server's flag `W3LPL` and `W3LPL-#` are
  /// indistinguishable here — and they are not the same kind of spot.
  let manualOnly = $state(false);
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
      await loadReference();
      const r = await api('GET', '/api/spots?limit=500');
      if (r.json?.spots) spots = r.json.spots;
      const s = await api('GET', '/api/status');
      status = s.json;
      const st = await api('GET', '/api/me/station');
      if (st.status === 200) station = st.json;
      const q = await api('GET', '/api/config/me/station');
      if (q.status === 200) locator = q.json?.locator ?? '';
    })();
    return openStream((frame) => {
      if (frame.type === 'spot') {
        spots = [frame.spot, ...spots].slice(0, MAX_ROWS);
      } else if (frame.type === 'status') {
        status = frame.status;
      }
    });
  });

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
      if (manualOnly && s.is_skimmer) return false;
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
  let maskedCount = $derived(visible.filter(masked).length);

  // Every narrowing the operator can be holding, including the older
  // source/CQ ones — the empty state has to account for all of them or it
  // will blame the wrong control.
  let narrowed = $derived(
    levelFilter.size > 0 ||
      modeFilter.size > 0 ||
      bandFilter.size > 0 ||
      sourceFilter.size > 0 ||
      cqOnly,
  );

  function clearFilters() {
    levelFilter = new Set();
    modeFilter = new Set();
    bandFilter = new Set();
    sourceFilter = new Set();
    cqOnly = false;
  }

  // Node state in the shared status-dot vocabulary: proven = up, connected
  // but nothing proven through it yet = amber, neither = down.
  const nodeDot = (n: any) =>
    n.proven ? 'on' : n.connected ? 'warn' : 'err';
</script>

<div class="page feedpage">
  <!-- The station card: whose log is driving the highlighting, and how far
       along it is. Worked/confirmed sit side by side because the gap between
       them IS the thing the ? levels exist to close. -->
  {#if station}
    <div class="card station">
      <div class="ident">
        <span class="call mono">{station.log_callsign ?? station.callsign}</span>
        {#if station.display_name}<span class="opname">{station.display_name}</span>{/if}
        {#if station.log_callsign && station.log_callsign !== station.callsign}
          <span class="hint">log · signed in as {station.callsign}</span>
        {/if}
      </div>
      {#if station.stats}
        <dl class="awards">
          <div>
            <dt>DXCC</dt>
            <dd><b>{shownStats.dxcc_worked}</b><span class="sep">/</span><span class="conf">{shownStats.dxcc_confirmed}</span></dd>
            <dd class="cap">worked / confirmed</dd>
          </div>
          <!-- Challenge sits next to DXCC, not next to Slots: it is an award
               total like DXCC is, and putting it beside the band×mode slot
               count is what makes people read the two as the same thing. -->
          <div title="DXCC Challenge: one point per entity per band over 160-6m (60m excluded, WARC included). Mode-agnostic. 1000 confirmed points to claim.">
            <dt>Challenge</dt>
            <dd><b>{shownStats.challenge_worked}</b><span class="sep">/</span><span class="conf">{shownStats.challenge_confirmed}</span></dd>
            <dd class="cap">worked / confirmed</dd>
          </div>
          <div title="Band x mode combinations. Distinct from Challenge points, which ignore mode and exclude 60m.">
            <dt>Slots</dt>
            <dd><b>{shownStats.slots_worked}</b><span class="sep">/</span><span class="conf">{shownStats.slots_confirmed}</span></dd>
            <dd class="cap">worked / confirmed</dd>
          </div>
          {#if station.qso_count}
            <div>
              <dt>QSOs</dt>
              <dd><b>{station.qso_count}</b></dd>
              <dd class="cap">refreshed {ago(station.last_refresh_unix)} ago</dd>
            </div>
          {/if}
        </dl>
        {#if canFilter(station.stats_current)}
          <label
            class="include-deleted"
            title="Totals count current DXCC entities by default, matching the ARRL standings. Tick to add the 62 deleted entities — Abu Ail, Blenheim Reef, British North Borneo and the rest. Those QSOs are in your log either way; they just score nothing."
          >
            <input type="checkbox" bind:checked={awards.includeDeleted} />include
            deleted
          </label>
        {/if}
      {:else}
        <span class="hint">
          No log loaded — set your ClubLog credentials in <b>My ClubLog</b> and
          refresh to get New/? highlighting.
        </span>
      {/if}
    </div>
  {/if}

  {#if status}
    <!-- Every cluster node also feeds `spots_per_source` (process_spot counts
         every spot by source name), so listing both maps flat printed each
         node twice with identical counts. Decoders are therefore the sources
         that are NOT nodes; a node's count lives in its own box, once. -->
    {@const nodeNames = new Set(Object.keys(status.cluster_nodes ?? {}))}
    {@const decoders = Object.entries(status.spots_per_source ?? {}).filter(
      ([name]) => !nodeNames.has(name),
    )}
    <div class="statusbar">
      <section class="statusbox">
        <h3>Decoders</h3>
        <div class="statusitems">
          {#each decoders as [name, count]}
            <span class="pill"><span class="status-dot on"></span>{name} <b>{count}</b></span>
          {:else}
            <span class="muted empty">nothing decoding</span>
          {/each}
        </div>
      </section>

      <section class="statusbox">
        <h3>Cluster nodes</h3>
        <div class="statusitems">
          {#each Object.entries(status.cluster_nodes ?? {}) as [name, n]}
            <span class="pill" title={n.state}>
              <span class="status-dot {nodeDot(n)}"></span>{name}
              <b>{n.spot_count}</b><span class="muted">{ago(n.last_spot_unix)}</span>
            </span>
          {:else}
            <span class="muted empty">none configured</span>
          {/each}
        </div>
      </section>

      <section class="statusbox">
        <h3>Feeds out</h3>
        <div class="statusitems">
          <span class="pill">TCP <b>{status.telnet_clients}</b></span>
          <span class="pill"
            >UDP <b>{status.udp_sent}</b>{#if status.udp_failed}<span class="err"
                >{status.udp_failed} fail</span
              >{/if}</span
          >
        </div>
      </section>

      <section class="statusbox">
        <h3>Reference</h3>
        <div class="statusitems">
          <span class="pill">cty <b>{status.cty_entities}</b></span>
          <span class="pill">LoTW <b>{status.lotw_users}</b></span>
        </div>
      </section>
    </div>
  {/if}

  <!-- Sources was a checkbox dropdown while every other narrowing on this
       screen was a chip row, so it hid both what was available and what was
       picked. Same ChipGroup as Alerts / Modes / Bands: All, then one chip
       per source, empty set meaning everything. -->
  <ChipGroup
    label="Sources"
    options={sourceNames.map((n) => ({ key: n, label: n }))}
    bind:selected={sourceFilter}
  />

  <div class="filters">
    <input
      class="search"
      type="search"
      placeholder="Search call or spotter"
      bind:value={search}
      aria-label="Filter spots by callsign or spotter"
    />
    {#if searchTerm}
      <button class="clear" onclick={() => (search = '')} title="Clear the search">clear</button>
    {/if}
    <label
      class="flabel"
      title="Hide spots made by skimmers (callsigns that arrived with the -# marker), leaving the ones a human typed."
      ><input type="checkbox" bind:checked={manualOnly} />Manual only</label
    >
    <label class="flabel"><input type="checkbox" bind:checked={cqOnly} />CQ only</label>
    <label class="flabel"><input type="checkbox" bind:checked={hideDupes} />Hide duplicates</label>
    <!-- Only offered once a locator exists, because without one the server
         sends no band advice and a permanently dead checkbox is worse than
         no checkbox. The route to it is the note beside the Locator field
         on My ClubLog. -->
    {#if locator}
      <label
        class="flabel"
        title="Recede spots on bands the sun says are not plausibly workable from {locator} right now. Nothing is hidden and New DXCC is never dimmed — see docs/PHASE-ROTATION-MASK.md."
        ><input type="checkbox" bind:checked={bandMask.on} />Band mask</label
      >
    {/if}
    <span class="count muted">{visible.length} spots</span>
    <!-- Never silent: a mask that changes the screen without saying so is
         indistinguishable from a feed going quiet. Nothing is removed in
         dim mode, so the count says "dimmed", not "hidden". -->
    {#if bandMask.on && maskedCount > 0}
      <span
        class="count masked-count"
        title="Dimmed, not hidden — every one of them is still in the table and still sortable. New DXCC is never dimmed."
        >{maskedCount} dimmed</span
      >
    {/if}
  </div>

  <!-- The three narrowings, one row each so a long band list wraps on its own
       line instead of shoving the others around. Remembered per browser. -->
  <div class="pickers">
    <ChipGroup label="Alerts" options={levels()} bind:selected={levelFilter} levelKeys />
    <ChipGroup label="Modes" options={modes()} bind:selected={modeFilter} />
    <ChipGroup label="Bands" options={bands()} bind:selected={bandFilter} />
  </div>

  <div class="card feed">
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th onclick={() => sortBy('time_unix')}>Time<i>{caret('time_unix')}</i></th>
            <th onclick={() => sortBy('source_name')}>Source<i>{caret('source_name')}</i></th>
            <th onclick={() => sortBy('spotter')}>Spotter<i>{caret('spotter')}</i></th>
            <th onclick={() => sortBy('dx_call')}>DX Call<i>{caret('dx_call')}</i></th>
            <th onclick={() => sortBy('freq')}>kHz<i>{caret('freq')}</i></th>
            <th onclick={() => sortBy('mode')}>Mode<i>{caret('mode')}</i></th>
            <th onclick={() => sortBy('snr_db')}>dB<i>{caret('snr_db')}</i></th>
            <th onclick={() => sortBy('band')}>Band<i>{caret('band')}</i></th>
            <th onclick={() => sortBy('dxcc_name')}>DXCC<i>{caret('dxcc_name')}</i></th>
            <th onclick={() => sortBy('alert')}>Alert<i>{caret('alert')}</i></th>
            <th>Message</th>
          </tr>
        </thead>
        <tbody>
          {#each visible as s}
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
              <td>{s.source_name}</td>
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
              <td class="mono call">
                {s.dx_call ?? '—'}{#if s.is_lotw}<span class="lotw" title="LoTW user">●</span>{/if}
              </td>
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
              <td>{s.dxcc_name ?? ''}</td>
              <td class="alert">{flagged(s) ? levelLabel(s.alert) : ''}</td>
              <!-- A cluster spot's `message` is synthesised; `comment` is
                   what the spotter actually typed, so prefer it. -->
              <td class="muted msg">{s.is_beacon ? '[BEACON] ' : ''}{s.comment || s.message}</td>
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

<style>
  /* The feed owns the window: the head keeps its own air and the table card
     takes whatever height is left, so the newest spot is never below the fold. */
  .feedpage {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  /* Status is four labelled boxes rather than one long pill run: the flat
     row put decoders, nodes, output counters and reference data on the same
     footing, so nothing told you which number belonged to which category. */
  .statusbar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0.6rem;
  }

  .statusbox {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--card-bg);
    padding: 0.45rem 0.65rem 0.55rem;
  }

  /* Natural width, no stretching: a box has exactly as much room as its
     contents need, so an idle Decoders box does not occupy a third of the
     bar announcing that nothing is decoding. The row wraps instead. */
  .statusbox {
    flex: 0 1 auto;
  }

  .statusbox h3 {
    margin: 0 0 0.35rem;
    font-size: 0.62rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .statusitems {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
  }

  .empty {
    font-size: 0.78rem;
  }

  /* --- Station card --- */
  .station {
    display: flex;
    align-items: center;
    gap: 2rem;
    flex-wrap: wrap;
    padding: 0.9rem 1.25rem;
  }

  .ident {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .station .call {
    font-size: 1.35rem;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .opname {
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  /* Sits with the totals it changes, not in the filter row — it is part of
     reading the card, not part of narrowing the feed. */
  /* After the numbers and hard right, not wedged between the callsign and
     the first total: the stat blocks are a rhythm of label/number/caption,
     and a checkbox dropped into the middle of it reads as a stray control.
     Subordinate on purpose — it changes the numbers, it is not one. */
  .include-deleted {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin-left: auto;
    color: var(--muted);
    font-size: 0.72rem;
    white-space: nowrap;
    cursor: pointer;
  }

  .awards {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 2rem;
    margin: 0;
  }

  .awards dt {
    font-size: var(--fs-hint);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .awards dd {
    margin: 0.1rem 0 0;
    font-size: 1.15rem;
    font-variant-numeric: tabular-nums;
  }

  /* Worked reads as the headline, confirmed as the qualifier under it —
     the gap between the two is what the ? levels exist to close. */
  .awards dd b {
    font-weight: 600;
  }

  .awards .sep {
    color: var(--muted);
    margin: 0 0.2rem;
    font-weight: 400;
  }

  .awards .conf {
    color: var(--ok);
  }

  .awards dd.cap {
    font-size: 0.7rem;
    color: var(--muted);
    letter-spacing: 0.02em;
  }

  .pickers {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
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

  /* One line: the two subset menus, then the boolean narrowings, then the
     count. Each stays a unit so a wrap breaks BETWEEN controls, not inside. */
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem 0.75rem;
  }

  /* Inherits the card/field vocabulary rather than inventing a control:
     same border, radius and focus ring as the System tab's inputs. */
  .search {
    font: inherit;
    font-size: 0.8rem;
    padding: 0.25rem 0.5rem;
    min-width: 12rem;
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
    margin-left: auto;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }

  /* The card holds the scroll rather than the page, so the header rule and
     the card's own edge stay put while the feed moves under them. */
  .feed {
    padding: 0;
    min-height: 14rem;
    overflow: hidden;
  }

  .table-wrap {
    overflow: auto;
    max-height: calc(100vh - 15rem);
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
     shove the row sideways. */
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

  /* The message is the widest column and the least urgent — it may run out
     rather than force the nine fixed columns off-screen. */
  .msg {
    max-width: 26rem;
    overflow: hidden;
    text-overflow: ellipsis;
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

  /* Sits beside the spot count, not in place of it — the operator needs
     both numbers to read the screen. Muted ink, because this is
     information about the view, not a condition to act on. */
  .masked-count {
    margin-left: 0.6rem;
    color: var(--muted);
    font-style: italic;
  }
</style>
