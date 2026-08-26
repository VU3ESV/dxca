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

  const nodeDot = (n: any) =>
    n.proven ? 'green' : n.connected ? 'yellow' : 'red';
</script>

{#if status}
  <div class="pills">
    {#each Object.entries(status.spots_per_source ?? {}) as [name, count]}
      <span class="pill"><span class="dot green"></span>{name} <b>{count}</b></span>
    {/each}
    {#each Object.entries(status.cluster_nodes ?? {}) as [name, n]}
      <span class="pill" title={n.state}>
        <span class="dot {nodeDot(n)}"></span>{name}
        <b>{n.spot_count}</b><span class="dim">{ago(n.last_spot_unix)}</span>
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
    <summary>Sources {sourceFilter.size ? `(${sourceFilter.size})` : ''}</summary>
    <div class="menu">
      {#each sourceNames as name}
        <label><input type="checkbox" checked={sourceFilter.has(name)}
          onchange={() => (sourceFilter = toggle(sourceFilter, name))} />{name}</label>
      {/each}
    </div>
  </details>
  <details>
    <summary>Bands {bandFilter.size ? `(${bandFilter.size})` : ''}</summary>
    <div class="menu">
      {#each bandNames as band}
        <label><input type="checkbox" checked={bandFilter.has(band)}
          onchange={() => (bandFilter = toggle(bandFilter, band))} />{band}</label>
      {/each}
    </div>
  </details>
  <label><input type="checkbox" bind:checked={newOnly} />New only</label>
  <label><input type="checkbox" bind:checked={cqOnly} />CQ only</label>
  <label><input type="checkbox" bind:checked={hideDupes} />Hide duplicates</label>
  <span class="dim">{visible.length} spots</span>
</div>

<div class="table-wrap">
  <table>
    <thead>
      <tr>
        <th onclick={() => sortBy('time_unix')}>Time</th>
        <th onclick={() => sortBy('source_name')}>Source</th>
        <th onclick={() => sortBy('dx_call')}>DX Call</th>
        <th onclick={() => sortBy('freq')}>kHz</th>
        <th onclick={() => sortBy('mode')}>Mode</th>
        <th onclick={() => sortBy('snr_db')}>dB</th>
        <th onclick={() => sortBy('band')}>Band</th>
        <th onclick={() => sortBy('dxcc_name')}>DXCC</th>
        <th onclick={() => sortBy('alert')}>Alert</th>
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
          <td>{s.mode}</td>
          <td class="mono">{s.snr_db}</td>
          <td>{s.band ?? ''}</td>
          <td>{s.dxcc_name ?? ''}</td>
          <td class="alert">{alertLabel[s.alert] ?? ''}</td>
          <td class="dim">{s.is_beacon ? '[BEACON] ' : ''}{s.message}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding: 10px 16px 0;
  }
  .filters {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
  }
  details {
    position: relative;
  }
  summary {
    cursor: pointer;
    color: var(--accent);
  }
  .menu {
    position: absolute;
    z-index: 10;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 140px;
  }
  .table-wrap {
    overflow: auto;
    max-height: calc(100vh - 160px);
    border-top: 1px solid var(--border);
  }
  .call {
    font-weight: 600;
  }
  .lotw {
    color: var(--green);
    margin-left: 3px;
    font-size: 9px;
    vertical-align: super;
  }
  .alert {
    font-weight: 600;
  }
  tr.a-dxcc td { background: var(--alert-dxcc); }
  tr.a-dxcc .alert { color: var(--red); }
  tr.a-slot td { background: var(--alert-slot); }
  tr.a-slot .alert { color: var(--orange); }
  tr.a-band td { background: var(--alert-band); }
  tr.a-band .alert { color: var(--accent); }
  tr.a-mode td { background: var(--alert-mode); }
  tr.a-mode .alert { color: var(--yellow); }
  tr.beacon td { color: var(--fg-dim); }
</style>
