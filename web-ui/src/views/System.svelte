<script lang="ts">
  // System view: live status detail plus — for admins — editing of
  // sources / nodes / destinations with hot-apply (the M5 remainder).
  // Bind-level scalars (ports, data dir) stay file-edited and show
  // read-only.
  import { api, ago } from '../lib/api';
  import { onMount } from 'svelte';

  let { isAdmin }: { isAdmin: boolean } = $props();
  let status = $state<any>(null);
  let cfg = $state<any>(null);
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  async function loadStatus() {
    const r = await api('GET', '/api/status');
    status = r.json;
  }
  async function loadConfig() {
    if (!isAdmin) return;
    const r = await api('GET', '/api/config/global');
    if (r.status === 200) cfg = r.json;
  }
  onMount(() => {
    loadStatus();
    loadConfig();
    const t = setInterval(loadStatus, 5000);
    return () => clearInterval(t);
  });

  async function saveConfig() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/global', {
      udp_sources: cfg.udp_sources,
      cluster_nodes: cfg.cluster_nodes,
      broadcast_destinations: cfg.broadcast_destinations,
    });
    busy = false;
    if (r.status === 200) {
      message = 'Applied live and saved to config/dxca.toml.';
      loadStatus();
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  async function refreshLotw() {
    busy = true; message = 'Downloading LoTW users list…'; error = '';
    const r = await api('POST', '/api/lotw/refresh');
    busy = false;
    if (r.status === 200) message = `LoTW list refreshed: ${r.json.lotw_users} users.`;
    else { message = ''; error = r.json?.error ?? `HTTP ${r.status}`; }
  }

  const addSource = () =>
    (cfg.udp_sources = [...cfg.udp_sources, { name: '', port: 2336, enabled: true }]);
  const addNode = () =>
    (cfg.cluster_nodes = [
      ...cfg.cluster_nodes,
      { name: '', host: '', port: 7300, login_call: '', password: '', enabled: true },
    ]);
  const addDest = () =>
    (cfg.broadcast_destinations = [
      ...cfg.broadcast_destinations,
      { name: '', ip: '127.0.0.1', port: 2237, format: 'passthrough', sources: [], unfiltered: false, enabled: true },
    ]);
  const drop = (list: any[], i: number) => list.filter((_, idx) => idx !== i);
</script>

<div class="page">
  {#if status}
    <div class="card wide">
      <h2>Server</h2>
      <p>
        dxca <code>v{status.version}</code> — {status.milestone} ·
        {status.users} user(s) · cty {status.cty_entities} entities ·
        LoTW {status.lotw_users} users · TCP clients {status.telnet_clients} ·
        UDP sent {status.udp_sent} / failed {status.udp_failed}
      </p>
      {#if isAdmin}
        <button onclick={refreshLotw} disabled={busy}>Refresh LoTW users list</button>
      {/if}
    </div>
    <div class="card wide">
      <h2>DX-cluster nodes — live</h2>
      <table>
        <thead><tr><th>Node</th><th>State</th><th>Spots</th><th>Last spot</th><th>Attempts</th></tr></thead>
        <tbody>
          {#each Object.entries(status.cluster_nodes ?? {}) as [name, n]}
            <tr>
              <td>{name}</td>
              <td>
                <span class="dot {n.proven ? 'green' : n.connected ? 'yellow' : 'red'}"></span>
                {n.state}
              </td>
              <td class="mono">{n.spot_count}</td>
              <td class="mono">{ago(n.last_spot_unix)}</td>
              <td class="mono">{n.attempt}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#if isAdmin && cfg}
    <div class="card wide">
      <h2>UDP sources</h2>
      <table>
        <thead><tr><th>Name</th><th>Port</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each cfg.udp_sources as s, i}
            <tr>
              <td><input bind:value={s.name} /></td>
              <td><input type="number" bind:value={s.port} class="port" /></td>
              <td><input type="checkbox" bind:checked={s.enabled} /></td>
              <td><button onclick={() => (cfg.udp_sources = drop(cfg.udp_sources, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <button onclick={addSource}>+ Add source</button>
    </div>

    <div class="card wide">
      <h2>DX-cluster nodes — config</h2>
      <table>
        <thead><tr><th>Name</th><th>Host</th><th>Port</th><th>Login</th><th>Password</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each cfg.cluster_nodes as n, i}
            <tr>
              <td><input bind:value={n.name} /></td>
              <td><input bind:value={n.host} class="host" /></td>
              <td><input type="number" bind:value={n.port} class="port" /></td>
              <td><input bind:value={n.login_call} class="call" /></td>
              <td><input type="password" bind:value={n.password} class="call" /></td>
              <td><input type="checkbox" bind:checked={n.enabled} /></td>
              <td><button onclick={() => (cfg.cluster_nodes = drop(cfg.cluster_nodes, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <button onclick={addNode}>+ Add node</button>
    </div>

    <div class="card wide">
      <h2>Broadcast destinations</h2>
      <table>
        <thead><tr><th>Name</th><th>IP</th><th>Port</th><th>Format</th><th>Sources (CSV, empty = all)</th><th>Unf</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each cfg.broadcast_destinations as d, i}
            <tr>
              <td><input bind:value={d.name} /></td>
              <td><input bind:value={d.ip} class="host" /></td>
              <td><input type="number" bind:value={d.port} class="port" /></td>
              <td>
                <select bind:value={d.format}>
                  <option value="cluster">cluster</option>
                  <option value="wsjtx">wsjtx</option>
                  <option value="passthrough">passthrough</option>
                </select>
              </td>
              <td>
                <input
                  value={d.sources.join(', ')}
                  onchange={(e: any) =>
                    (d.sources = e.target.value.split(',').map((s: string) => s.trim()).filter(Boolean))}
                />
              </td>
              <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
              <td><input type="checkbox" bind:checked={d.enabled} /></td>
              <td><button onclick={() => (cfg.broadcast_destinations = drop(cfg.broadcast_destinations, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <button onclick={addDest}>+ Add destination</button>
    </div>

    <div class="card wide">
      <div class="actions">
        <button class="primary" onclick={saveConfig} disabled={busy}>Apply &amp; save</button>
        <span class="dim">
          Applies live (listeners rebind, nodes redial, destinations re-point)
          and rewrites config/dxca.toml.
        </span>
      </div>
      {#if message}<p class="ok">{message}</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}
      <p class="dim">
        File-only settings: web {cfg.read_only.web_bind} · telnet
        {cfg.read_only.telnet_port} · dedupe {cfg.read_only.dedupe_window_secs}s ·
        ring {cfg.read_only.spot_ring_capacity} · data dir
        <code>{cfg.read_only.data_dir}</code> (edit config/dxca.toml + restart).
      </p>
    </div>
  {/if}
</div>

<style>
  .page { padding: 20px; display: flex; flex-direction: column; gap: 16px; }
  .card.wide { max-width: 860px; }
  th { position: static; }
  td input:not([type='checkbox']) { width: 100%; min-width: 70px; }
  td input.port { max-width: 80px; }
  td input.host { min-width: 130px; }
  td input.call { min-width: 90px; }
  .actions { display: flex; align-items: center; gap: 14px; }
</style>
