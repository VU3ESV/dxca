<script lang="ts">
  // System view: sources/nodes/destination status detail + LoTW refresh.
  // Config *editing* with hot-apply is the remaining M5 item (HANDOVER).
  import { api, ago } from '../lib/api';
  import { onMount } from 'svelte';

  let { isAdmin }: { isAdmin: boolean } = $props();
  let status = $state<any>(null);
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  async function load() {
    const r = await api('GET', '/api/status');
    status = r.json;
  }
  onMount(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  });

  async function refreshLotw() {
    busy = true; message = 'Downloading LoTW users list…'; error = '';
    const r = await api('POST', '/api/lotw/refresh');
    busy = false;
    if (r.status === 200) message = `LoTW list refreshed: ${r.json.lotw_users} users.`;
    else { message = ''; error = r.json?.error ?? `HTTP ${r.status}`; }
  }
</script>

<div class="page">
  {#if status}
    <div class="card">
      <h2>Server</h2>
      <p>
        dxca <code>v{status.version}</code> — {status.milestone} ·
        {status.users} user(s) · cty {status.cty_entities} entities ·
        LoTW {status.lotw_users} users · TCP clients {status.telnet_clients} ·
        UDP sent {status.udp_sent} / failed {status.udp_failed}
      </p>
      {#if isAdmin}
        <button onclick={refreshLotw} disabled={busy}>Refresh LoTW users list</button>
        {#if message}<p class="ok">{message}</p>{/if}
        {#if error}<p class="err">{error}</p>{/if}
      {/if}
    </div>
    <div class="card">
      <h2>UDP sources</h2>
      <table>
        <thead><tr><th>Source</th><th>Spots</th></tr></thead>
        <tbody>
          {#each Object.entries(status.spots_per_source ?? {}) as [name, count]}
            <tr><td>{name}</td><td class="mono">{count}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="card">
      <h2>DX-cluster nodes</h2>
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
      <p class="dim">
        Sources, nodes, and destinations are configured in
        <code>config/dxca.toml</code> for now — web editing lands with the
        M5 remainder.
      </p>
    </div>
  {/if}
</div>

<style>
  .page { padding: 20px; display: flex; flex-direction: column; gap: 16px; }
  .card { max-width: 720px; }
  th { position: static; }
</style>
