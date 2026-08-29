<script lang="ts">
  // Settings › Server › MQTT.
  //
  // Its own endpoint and its own Save, not the global config's: these rows
  // carry a broker password, so they live in the 0600 database rather than in
  // config/dxca.toml, which is installed world-readable.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  let mqtt = $state<any[]>([]);
  let stats = $state<any>(null);
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  async function load() {
    const r = await api('GET', '/api/mqtt');
    if (r.status === 200) {
      mqtt = r.json.destinations ?? [];
      stats = r.json;
    }
  }

  /// Counters only. Deliberately does NOT touch `mqtt`: that array is bound to
  /// the inputs, so refreshing it on a timer would wipe whatever the operator
  /// was halfway through typing.
  async function loadStats() {
    const r = await api('GET', '/api/mqtt');
    if (r.status === 200) stats = r.json;
  }

  onMount(() => {
    load();
    // Polled with the rest of the status, not only on save — otherwise
    // "published 0, failed 0" sits there until a page reload while spots are
    // in fact flowing.
    const t = setInterval(loadStats, 5000);
    return () => clearInterval(t);
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/mqtt', { destinations: mqtt });
    if (r.status === 200) {
      mqtt = r.json.destinations;
      message = `Saved — ${r.json.connected} destination(s) connecting.`;
      await load();
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
    busy = false;
  }

  const add = () =>
    (mqtt = [
      ...mqtt,
      {
        name: '', host: '192.168.1.169', port: 1883, username: '', password: '',
        topic: 'shack/dxca/spots', client_id: 'dxca',
        sources: [], unfiltered: false, enabled: true,
      },
    ]);

  const dropRow = (i: number) => (mqtt = mqtt.filter((_, idx) => idx !== i));
</script>

<div class="card">
  <h2>
    MQTT destinations
    <HelpTip label="MQTT destinations">
      <span class="para">
        Each spot is published twice, to sibling topics under the base:
        <code>&lt;topic&gt;/json</code> carries the structured spot (callsign,
        frequency, band, mode, SNR, comment) and
        <code>&lt;topic&gt;/cluster</code> carries the plain DX-cluster line.
      </span>
      <span class="para">
        Plain MQTT on 1883 with optional username/password — TLS is not built
        in. Credentials are stored in <code>data/dxca.db</code> (0600), never in
        <code>config/dxca.toml</code>, which is why this page has its own Save.
      </span>
    </HelpTip>
  </h2>
  <div class="editor-scroll">
    <table class="editor">
      <thead>
        <tr>
          <th>Name</th><th>Broker</th><th>Port</th><th>User</th><th>Password</th>
          <th>Base topic</th><th>Client ID</th><th>Sources (CSV, empty = all)</th>
          <th>Unf</th><th>On</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each mqtt as d, i}
          <tr>
            <td><input bind:value={d.name} /></td>
            <td><input bind:value={d.host} class="host" /></td>
            <td><input type="number" bind:value={d.port} class="port" /></td>
            <td><input bind:value={d.username} class="port" /></td>
            <td><input type="password" bind:value={d.password} class="port" /></td>
            <td><input bind:value={d.topic} class="host" /></td>
            <td><input bind:value={d.client_id} class="port" /></td>
            <td>
              <input
                class="csv"
                value={d.sources.join(', ')}
                onchange={(e: any) =>
                  (d.sources = e.target.value.split(',').map((x: string) => x.trim()).filter(Boolean))}
              />
            </td>
            <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
            <td><input type="checkbox" bind:checked={d.enabled} /></td>
            <td><button class="drop" title="Remove" onclick={() => dropRow(i)}>✕</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <div class="actions">
    <button onclick={add}>+ Add MQTT destination</button>
    <button class="primary" onclick={save} disabled={busy}>Apply &amp; save</button>
    {#if stats}
      <span class="hint">published {stats.sent ?? 0}, failed {stats.failed ?? 0}</span>
    {/if}
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  p {
    margin: 0.75rem 0 0;
  }
</style>
