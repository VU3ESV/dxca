<script lang="ts">
  // The spots dashboard: status pills + live table (plan §8 page 2).
  // Backfills over /api/spots, then rides /api/stream; filters and the
  // 60 s duplicate collapse mirror the 1.x display behaviour.
  import { api, openStream, hhmm, ago } from '../lib/api';
  import { onMount } from 'svelte';

  let spots = $state<any[]>([]);
  let status = $state<any>(null);
  let sortKey = $state('time_unix');
  let sortDesc = $state(true);
  let newOnly = $state(false);
  let cqOnly = $state(false);
  let hideDupes = $state(true);
  let sourceFilter = $state<Set<string>>(new Set());
  let bandFilter = $state<Set<string>>(new Set());
  const MAX_ROWS = 1500;

  onMount(() => {
    (async () => {
      const r = await api('GET', '/api/spots?limit=500');
      if (r.json?.spots) spots = r.json.spots;
      const s = await api('GET', '/api/status');
      status = s.json;
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
  const isNew = (s: any) =>
    ['newDXCC', 'newSlot', 'newBand', 'newMode'].includes(s.alert);

  let sourceNames = $derived(
    Object.keys(status?.spots_per_source ?? {}).sort(),
  );
  let bandNames = $derived(
    [...new Set(spots.map((s) => s.band).filter(Boolean))].sort(),
  );

  let visible = $derived.by(() => {
    let rows = spots.filter((s) => {
      if (cqOnly && !s.message?.toUpperCase().startsWith('CQ ')) return false;
      if (newOnly && !isNew(s)) return false;
      if (sourceFilter.size && !sourceFilter.has(s.source_name)) return false;
      if (bandFilter.size && (!s.band || !bandFilter.has(s.band))) return false;
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

  const alertLabel: Record<string, string> = {
    newDXCC: 'NEW DXCC', newSlot: 'New Slot', newBand: 'New Band',
    newMode: 'New Mode', worked: '', none: '',
  };
  const rowClass = (s: any) =>
    s.alert === 'newDXCC' ? 'a-dxcc'
    : s.alert === 'newSlot' ? 'a-slot'
    : s.alert === 'newBand' ? 'a-band'
    : s.alert === 'newMode' ? 'a-mode'
    : s.is_beacon ? 'beacon' : '';

  // Node state in the shared status-dot vocabulary: proven = up, connected
  // but nothing proven through it yet = amber, neither = down.
  const nodeDot = (n: any) =>
    n.proven ? 'on' : n.connected ? 'warn' : 'err';
</script>

<div class="page feedpage">
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
    <details>
      <summary class="filter-chip" class:on={bandFilter.size}>
        Bands {bandFilter.size ? `(${bandFilter.size})` : ''}
      </summary>
      <div class="menu">
        {#each bandNames as band}
          <label><input type="checkbox" checked={bandFilter.has(band)}
            onchange={() => (bandFilter = toggle(bandFilter, band))} />{band}</label>
        {/each}
      </div>
    </details>
    <span class="fsep"></span>
    <label class="flabel"><input type="checkbox" bind:checked={newOnly} />New only</label>
    <label class="flabel"><input type="checkbox" bind:checked={cqOnly} />CQ only</label>
    <label class="flabel"><input type="checkbox" bind:checked={hideDupes} />Hide duplicates</label>
    <span class="count muted">{visible.length} spots</span>
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
            <tr class={rowClass(s)}>
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
              <td class="alert">{alertLabel[s.alert] ?? ''}</td>
              <td class="muted msg">{s.is_beacon ? '[BEACON] ' : ''}{s.message}</td>
            </tr>
          {/each}
        </tbody>
      </table>
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

  /* Level colour and row wash come from the same `--alert-*` token, so the
     cell and its tint can never disagree. */
  tr.a-dxcc td { background: var(--alert-dxcc-bg); }
  tr.a-dxcc .alert { color: var(--alert-dxcc); }
  tr.a-slot td { background: var(--alert-slot-bg); }
  tr.a-slot .alert { color: var(--alert-slot); }
  tr.a-band td { background: var(--alert-band-bg); }
  tr.a-band .alert { color: var(--alert-band); }
  tr.a-mode td { background: var(--alert-mode-bg); }
  tr.a-mode .alert { color: var(--alert-mode); }
  tr.beacon td { color: var(--muted); }
</style>
