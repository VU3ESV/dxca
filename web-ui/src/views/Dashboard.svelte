<script lang="ts">
  // The spots dashboard: status pills + live table (plan §8 page 2).
  // Backfills over /api/spots, then rides /api/stream; filters and the
  // 60 s duplicate collapse mirror the 1.x display behaviour.
  import { api, openStream, hhmm, ago } from '../lib/api';
  import { onMount } from 'svelte';
  import ChipGroup from '../lib/ChipGroup.svelte';
  import { loadReference, bands, modes, levels, levelLabel } from '../lib/reference.svelte';

  let spots = $state<any[]>([]);
  let status = $state<any>(null);
  let station = $state<any>(null);
  let sortKey = $state('time_unix');
  let sortDesc = $state(true);
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
  // dxca-core's modes::canonical so the display filter agrees with the
  // Telegram gate; anything unrecognised is DATA, exactly as there.
  const PHONE = ['SSB', 'USB', 'LSB', 'AM', 'FM', 'PHONE', 'VOICE', 'DIGITALVOICE', 'C4FM', 'DMR', 'DSTAR'];
  function modeClass(mode: string | null | undefined): string {
    const m = (mode ?? '').trim().toUpperCase();
    if (m === 'CW') return 'CW';
    return PHONE.includes(m) ? 'PHONE' : 'DATA';
  }

  let sourceNames = $derived(
    Object.keys(status?.spots_per_source ?? {}).sort(),
  );

  let visible = $derived.by(() => {
    let rows = spots.filter((s) => {
      if (cqOnly && !s.message?.toUpperCase().startsWith('CQ ')) return false;
      if (sourceFilter.size && !sourceFilter.has(s.source_name)) return false;
      if (bandFilter.size && (!s.band || !bandFilter.has(s.band))) return false;
      if (modeFilter.size && !modeFilter.has(modeClass(s.mode))) return false;
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

  function toggle(set: Set<string>, value: string) {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  }

  // The sort marker rides the active column only — a caret on every header
  // makes the row look like ten controls instead of one answer.
  const caret = (key: string) =>
    sortKey === key ? (sortDesc ? '↓' : '↑') : '';

  // Both the label and the colour now come from one place: the label from
  // the server's own AlertLevel::label() via /api/reference, the colour from
  // app.css's [data-level] table. Adding a ninth level needs no edit here.
  const flagged = (s: any) => s.alert && s.alert !== 'worked' && s.alert !== 'none';

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
            <dd><b>{station.stats.dxcc_worked}</b><span class="sep">/</span><span class="conf">{station.stats.dxcc_confirmed}</span></dd>
            <dd class="cap">worked / confirmed</dd>
          </div>
          <div>
            <dt>Slots</dt>
            <dd><b>{station.stats.slots_worked}</b><span class="sep">/</span><span class="conf">{station.stats.slots_confirmed}</span></dd>
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
      {:else}
        <span class="hint">
          No log loaded — set your ClubLog credentials in <b>My ClubLog</b> and
          refresh to get New/? highlighting.
        </span>
      {/if}
    </div>
  {/if}

  {#if status}
    <div class="pills">
      {#each Object.entries(status.spots_per_source ?? {}) as [name, count]}
        <span class="pill"><span class="status-dot on"></span>{name} <b>{count}</b></span>
      {/each}
      {#each Object.entries(status.cluster_nodes ?? {}) as [name, n]}
        <span class="pill" title={n.state}>
          <span class="status-dot {nodeDot(n)}"></span>{name}
          <b>{n.spot_count}</b><span class="muted">{ago(n.last_spot_unix)}</span>
        </span>
      {/each}
      <span class="pill">TCP <b>{status.telnet_clients}</b></span>
      <span class="pill">UDP→ <b>{status.udp_sent}</b>{#if status.udp_failed}<span class="err">({status.udp_failed} fail)</span>{/if}</span>
      <span class="pill">cty <b>{status.cty_entities}</b></span>
      <span class="pill">LoTW <b>{status.lotw_users}</b></span>
    </div>
  {/if}

  <div class="filters">
    <details>
      <summary class="filter-chip" class:on={sourceFilter.size}>
        Sources {sourceFilter.size ? `(${sourceFilter.size})` : ''}
      </summary>
      <div class="menu">
        {#each sourceNames as name}
          <label><input type="checkbox" checked={sourceFilter.has(name)}
            onchange={() => (sourceFilter = toggle(sourceFilter, name))} />{name}</label>
        {/each}
      </div>
    </details>
    <span class="fsep"></span>
    <label class="flabel"><input type="checkbox" bind:checked={cqOnly} />CQ only</label>
    <label class="flabel"><input type="checkbox" bind:checked={hideDupes} />Hide duplicates</label>
    <span class="count muted">{visible.length} spots</span>
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
              data-level={flagged(s) ? s.alert : undefined}
            >
              <td class="mono">{hhmm(s.time_unix)}Z</td>
              <td>{s.source_name}</td>
              <td class="mono call">
                {s.dx_call ?? '—'}{#if s.is_lotw}<span class="lotw" title="LoTW user">●</span>{/if}
              </td>
              <td class="mono">{freqKHz(s).toFixed(1)}</td>
              <td class="mode">{s.mode}</td>
              <td class="mono">{s.snr_db}</td>
              <td>{s.band ?? ''}</td>
              <td>{s.dxcc_name ?? ''}</td>
              <td class="alert">{flagged(s) ? levelLabel(s.alert) : ''}</td>
              <td class="muted msg">{s.is_beacon ? '[BEACON] ' : ''}{s.message}</td>
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

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
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

  .fsep {
    width: 1px;
    height: 1.1rem;
    background: var(--border);
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

  details {
    position: relative;
  }

  /* The chip IS the disclosure trigger, so the native marker would sit inside
     the pill as a second affordance. */
  summary {
    list-style: none;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  summary.on {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .menu {
    position: absolute;
    z-index: 10;
    margin-top: 0.35rem;
    background: Canvas;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 9rem;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.25);
  }

  .menu label {
    font-size: 0.85rem;
    padding: 0.15rem 0.3rem;
    border-radius: 4px;
  }

  .menu label:hover {
    background: var(--card-bg);
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
</style>
