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
    loadMqtt();
    // MQTT counters are polled with the rest of the status, not only on save
    // — otherwise "published 0, failed 0" sits there until a page reload
    // while spots are in fact flowing.
    const t = setInterval(() => {
      loadStatus();
      loadMqttStats();
    }, 5000);
    return () => clearInterval(t);
  });

  async function saveConfig() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/global', {
      udp_sources: cfg.udp_sources,
      cluster_nodes: cfg.cluster_nodes,
      broadcast_destinations: cfg.broadcast_destinations,
      // Sent every save. The server treats an ABSENT field as "leave as-is"
      // and an empty string as a deliberate clear, so always sending the
      // current value keeps this box the source of truth.
      clublog_api_key: cfg.clublog_api_key ?? '',
    });
    busy = false;
    if (r.status === 200) {
      message = 'Applied live and saved to config/dxca.toml.';
      loadStatus();
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  async function refreshCty() {
    busy = true; message = 'Downloading cty.xml from ClubLog...'; error = '';
    const r = await api('POST', '/api/cty/refresh');
    busy = false;
    if (r.status === 200) {
      message = `cty.xml refreshed: ${r.json.cty_entities} entities.`;
      loadStatus();
      loadConfig();
    } else { message = ''; error = r.json?.error ?? `HTTP ${r.status}`; }
  }

  async function refreshLotw() {
    busy = true; message = 'Downloading LoTW users list…'; error = '';
    const r = await api('POST', '/api/lotw/refresh');
    busy = false;
    if (r.status === 200) { message = `LoTW list refreshed: ${r.json.lotw_users} users.`; loadConfig(); }
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

  // MQTT destinations have their own endpoint, not /api/config/global: they
  // carry a broker password and so live in the 0600 database rather than in
  // config/dxca.toml, which is installed world-readable.
  let mqtt = $state<any[]>([]);
  let mqttStats = $state<any>(null);
  let mqttMessage = $state('');
  let mqttError = $state('');
  let mqttBusy = $state(false);

  async function loadMqtt() {
    if (!isAdmin) return;
    const r = await api('GET', '/api/mqtt');
    if (r.status === 200) {
      mqtt = r.json.destinations ?? [];
      mqttStats = r.json;
    }
  }

  /// Counters only. Deliberately does NOT touch `mqtt`: that array is bound
  /// to the inputs, so refreshing it on a timer would wipe whatever the
  /// operator was halfway through typing.
  async function loadMqttStats() {
    if (!isAdmin) return;
    const r = await api('GET', '/api/mqtt');
    if (r.status === 200) mqttStats = r.json;
  }

  async function saveMqtt() {
    mqttBusy = true; mqttMessage = ''; mqttError = '';
    const r = await api('PUT', '/api/mqtt', { destinations: mqtt });
    if (r.status === 200) {
      mqtt = r.json.destinations;
      mqttMessage = `Saved — ${r.json.connected} destination(s) connecting.`;
      await loadMqtt();
    } else {
      mqttError = r.json?.error ?? `HTTP ${r.status}`;
    }
    mqttBusy = false;
  }

  const addMqtt = () =>
    (mqtt = [
      ...mqtt,
      {
        name: '',
        host: '192.168.1.169',
        port: 1883,
        username: '',
        password: '',
        topic: 'shack/dxca/spots',
        client_id: 'dxca',
        sources: [],
        unfiltered: false,
        enabled: true,
      },
    ]);
</script>

<div class="page stack">
  {#if status}
    <!-- The server's own facts, as a labelled stat list rather than a run-on
         sentence: each is a number an operator scans for, not prose. -->
    <div class="card">
      <h2>Server</h2>
      <dl class="stats">
        <div><dt>Version</dt><dd class="mono">v{status.version}</dd></div>
        <div><dt>Milestone</dt><dd>{status.milestone}</dd></div>
        <div><dt>Users</dt><dd class="num">{status.users}</dd></div>
        <div><dt>cty entities</dt><dd class="num">{status.cty_entities}</dd></div>
        <div><dt>LoTW users</dt><dd class="num">{status.lotw_users}</dd></div>
        <div><dt>TCP clients</dt><dd class="num">{status.telnet_clients}</dd></div>
        <div><dt>UDP sent</dt><dd class="num">{status.udp_sent}</dd></div>
        <div>
          <dt>UDP failed</dt>
          <dd class="num" class:err={status.udp_failed}>{status.udp_failed}</dd>
        </div>
      </dl>
    </div>

    <!-- The two SHARED reference datasets. Both are one file backing one
         in-memory structure that every account reads, so both are admin-only
         and both refresh server-wide — unlike a ClubLog log, which is per
         account. They live together because that symmetry is the point. -->
    {#if isAdmin && cfg}
      <div class="card">
        <h2>Reference data — shared by all users</h2>

        <div class="settings-form">
          <span class="label">ClubLog API key</span>
          <input
            type="password"
            bind:value={cfg.clublog_api_key}
            placeholder="from clublog.org — one key for the whole server"
          />
        </div>
        <p class="hint note">
          Fetches <b>cty.xml</b>, the DXCC prefix database every account is
          classified against — so it belongs to the server, not to an
          operator. It is <b>not</b> used to download anyone's log; that uses
          each user's own email and app password in <b>My ClubLog</b>.
          Saved with <b>Apply &amp; save</b> below.
        </p>

        <table class="refdata">
          <tbody>
            <tr>
              <td class="what">cty.xml<br /><span class="hint">{status?.cty_entities ?? '—'} entities</span></td>
              <td class="when hint">
                {#if cfg.read_only.cty_refresh_days}
                  auto every {cfg.read_only.cty_refresh_days}d ·
                {:else}
                  auto off ·
                {/if}
                {#if cfg.cty_last_refresh_unix}
                  last {ago(cfg.cty_last_refresh_unix)} ago
                {:else}
                  never downloaded here
                {/if}
              </td>
              <td><button onclick={refreshCty} disabled={busy || !cfg.clublog_api_key}>Refresh now</button></td>
            </tr>
            <tr>
              <td class="what">LoTW users<br /><span class="hint">{status?.lotw_users ?? '—'} calls</span></td>
              <td class="when hint">
                {#if cfg.read_only.lotw_refresh_days}
                  auto every {cfg.read_only.lotw_refresh_days}d ·
                {:else}
                  auto off ·
                {/if}
                {#if cfg.lotw_last_refresh_unix}
                  last {ago(cfg.lotw_last_refresh_unix)} ago
                {:else}
                  never downloaded here
                {/if}
              </td>
              <td><button onclick={refreshLotw} disabled={busy}>Refresh now</button></td>
            </tr>
          </tbody>
        </table>
      </div>
    {/if}

    <div class="card">
      <h2>DX-cluster nodes — live</h2>
      <table>
        <thead><tr><th>Node</th><th>State</th><th>Spots</th><th>Last spot</th><th>Attempts</th></tr></thead>
        <tbody>
          {#each Object.entries(status.cluster_nodes ?? {}) as [name, n]}
            <tr>
              <td>{name}</td>
              <td>
                <span class="status-dot {n.proven ? 'on' : n.connected ? 'warn' : 'err'}"></span>
                {n.state}
              </td>
              <td class="num">{n.spot_count}</td>
              <td class="num">{ago(n.last_spot_unix)}</td>
              <td class="num">{n.attempt}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#if isAdmin && cfg}
    <div class="card">
      <h2>UDP sources</h2>
      <div class="editor-scroll">
      <table class="editor">
        <thead><tr><th>Name</th><th>Port</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each cfg.udp_sources as s, i}
            <tr>
              <td><input bind:value={s.name} /></td>
              <td><input type="number" bind:value={s.port} class="port" /></td>
              <td><input type="checkbox" bind:checked={s.enabled} /></td>
              <td><button class="drop" title="Remove" onclick={() => (cfg.udp_sources = drop(cfg.udp_sources, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
      <div class="actions"><button onclick={addSource}>+ Add source</button></div>
    </div>

    <div class="card">
      <h2>DX-cluster nodes — config</h2>
      <div class="editor-scroll">
      <table class="editor">
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
              <td><button class="drop" title="Remove" onclick={() => (cfg.cluster_nodes = drop(cfg.cluster_nodes, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
      <div class="actions"><button onclick={addNode}>+ Add node</button></div>
    </div>

    <div class="card">
      <h2>Broadcast destinations</h2>
      <div class="editor-scroll">
      <table class="editor">
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
                  class="csv"
                  value={d.sources.join(', ')}
                  onchange={(e: any) =>
                    (d.sources = e.target.value.split(',').map((s: string) => s.trim()).filter(Boolean))}
                />
              </td>
              <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
              <td><input type="checkbox" bind:checked={d.enabled} /></td>
              <td><button class="drop" title="Remove" onclick={() => (cfg.broadcast_destinations = drop(cfg.broadcast_destinations, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
      <div class="actions"><button onclick={addDest}>+ Add destination</button></div>
    </div>

    <!-- Saved and applied on its own button, not the config one: these rows
         are stored in the database rather than config/dxca.toml, because a
         broker password does not belong in a 0644 file. -->
    <div class="card">
      <h2>MQTT destinations</h2>
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
                    (d.sources = e.target.value.split(',').map((s: string) => s.trim()).filter(Boolean))}
                />
              </td>
              <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
              <td><input type="checkbox" bind:checked={d.enabled} /></td>
              <td><button class="drop" title="Remove" onclick={() => (mqtt = drop(mqtt, i))}>✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
      <div class="actions">
        <button onclick={addMqtt}>+ Add MQTT destination</button>
        <button class="primary" onclick={saveMqtt} disabled={mqttBusy}>Apply &amp; save</button>
        {#if mqttStats}
          <span class="hint">published {mqttStats.sent ?? 0}, failed {mqttStats.failed ?? 0}</span>
        {/if}
      </div>
      <p class="hint">
        Each spot is published twice, to sibling topics under the base:
        <code>&lt;topic&gt;/json</code> carries the structured spot (callsign,
        frequency, band, mode, SNR, comment) and <code>&lt;topic&gt;/cluster</code>
        carries the plain DX-cluster line. Plain MQTT on 1883 with optional
        username/password — TLS is not built in. Credentials are stored in
        <code>data/dxca.db</code> (0600), never in <code>config/dxca.toml</code>.
      </p>
      {#if mqttMessage}<p class="ok">{mqttMessage}</p>{/if}
      {#if mqttError}<p class="err">{mqttError}</p>{/if}
    </div>

    <div class="card">
      <div class="actions apply">
        <button class="primary" onclick={saveConfig} disabled={busy}>Apply &amp; save</button>
        <span class="hint">
          Applies live (listeners rebind, nodes redial, destinations re-point)
          and rewrites config/dxca.toml.
        </span>
      </div>
      {#if message}<p class="ok">{message}</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}
      <p class="hint file-only">
        File-only settings: web {cfg.read_only.web_bind} · telnet
        {cfg.read_only.telnet_port} · dedupe {cfg.read_only.dedupe_window_secs}s ·
        ring {cfg.read_only.spot_ring_capacity} · cty refresh
        {cfg.read_only.cty_refresh_days}d · LoTW refresh
        {cfg.read_only.lotw_refresh_days}d · data dir
        <code>{cfg.read_only.data_dir}</code> (edit config/dxca.toml + restart).
      </p>
    </div>
  {/if}
</div>

<style>
  /* One column of full-width cards: every card here is a wide table or an
     editor grid, so the masonry columns the settings screens use would only
     squeeze them. */
  .stack {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    max-width: 66rem;
  }

  /* Label over value, wrapping into as many columns as fit. */
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

  .stats dd.err {
    color: var(--err);
  }

  /* Sized to its fields, not to the page: at `width: 100%` the table hands
     all the slack to the first column and a source name gets a 30rem box. */
  /* An editor row wider than the window scrolls inside its own card instead
     of dragging the whole page sideways. The MQTT row is eleven columns —
     broker, port, user, password, topic, client id, sources — and comes to
     roughly 77rem, which overflows a 1280px laptop outright. */
  .editor-scroll {
    overflow-x: auto;
  }

  .editor {
    width: auto;
  }

  /* An editor row is fields, not readings: it gets air the dense feed table
     deliberately refuses, and no rule under each row to fight the inputs. */
  .editor td {
    padding: 0.25rem 0.5rem 0.25rem 0;
    border-bottom: none;
  }

  .editor td input:not([type='checkbox']),
  .editor td select {
    width: 9rem;
  }

  .editor td input.port { width: 5rem; }
  .editor td input.host { width: 12rem; }
  .editor td input.call { width: 7rem; }
  .editor td input.csv { width: 14rem; }

  /* Square, quiet, and only red on approach — a delete that shouts from rest
     turns every row into a warning. */
  .drop {
    padding: 0.15rem 0.45rem;
    color: var(--muted);
  }

  .drop:hover {
    color: var(--err);
    border-color: var(--err);
  }

  .apply {
    margin-top: 0;
  }

  .note {
    margin: 0.5rem 0 1rem;
    line-height: 1.5;
    max-width: 40rem;
  }

  /* Two shared datasets, three columns: what, when, act. */
  .refdata {
    width: auto;
  }

  .refdata td {
    padding: 0.35rem 1.25rem 0.35rem 0;
    vertical-align: middle;
  }

  .refdata .what {
    line-height: 1.35;
  }

  .refdata .when {
    white-space: nowrap;
  }

  .file-only {
    margin: 0.75rem 0 0;
    line-height: 1.5;
  }
</style>
