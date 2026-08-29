<script lang="ts">
  // Settings › Server › Reference data — the server-wide lists every account is
  // read against, plus the server's own facts.
  //
  // The call blacklist is here rather than under Access because it is the same
  // KIND of thing as cty.xml and the LoTW list: one list, server-wide, that
  // every account is subject to whether or not they know it exists. Access is
  // about who may log in, which the blacklist has nothing to do with.
  //
  // Both are one file backing one in-memory structure that every account
  // shares, so both are admin-only and both refresh server-wide — unlike a
  // ClubLog log, which is per account. They live together because that
  // symmetry is the point.
  import { api, ago } from '../../lib/api';
  import { onMount } from 'svelte';
  import { status, refreshStatus } from '../../lib/status.svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ApplySave from '../../lib/ApplySave.svelte';
  import Blacklist from '../Blacklist.svelte';
  import { server, loadServerConfig } from '../../lib/serverconfig.svelte';

  let s = $derived(status());
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(() => {
    loadServerConfig();
    refreshStatus();
  });

  async function refreshCty() {
    busy = true; message = 'Downloading cty.xml from ClubLog…'; error = '';
    const r = await api('POST', '/api/cty/refresh');
    busy = false;
    if (r.status === 200) {
      message = `cty.xml refreshed: ${r.json.cty_entities} entities.`;
      await refreshStatus();
      // The download rewrites the stored timestamp, so the config we are
      // showing is stale the moment it succeeds.
      await loadServerConfig(true);
    } else { message = ''; error = r.json?.error ?? `HTTP ${r.status}`; }
  }

  async function refreshLotw() {
    busy = true; message = 'Downloading LoTW users list…'; error = '';
    const r = await api('POST', '/api/lotw/refresh');
    busy = false;
    if (r.status === 200) {
      message = `LoTW list refreshed: ${r.json.lotw_users} users.`;
      await refreshStatus();
      await loadServerConfig(true);
    } else { message = ''; error = r.json?.error ?? `HTTP ${r.status}`; }
  }
</script>

{#if s}
  <div class="card">
    <h2>Server</h2>
    <dl class="stats">
      <div><dt>Version</dt><dd class="mono">v{s.version}</dd></div>
      <div><dt>Milestone</dt><dd>{s.milestone}</dd></div>
      <div><dt>Users</dt><dd class="num">{s.users}</dd></div>
      <div><dt>TCP clients</dt><dd class="num">{s.telnet_clients}</dd></div>
      <div><dt>UDP sent</dt><dd class="num">{s.udp_sent}</dd></div>
      <div>
        <dt>UDP failed</dt>
        <dd class="num" class:err={s.udp_failed}>{s.udp_failed}</dd>
      </div>
    </dl>
  </div>
{/if}

{#if server.cfg}
  <div class="card">
    <h2>Reference data — shared by all users</h2>

    <div class="settings-form">
      <span class="label">
        ClubLog API key
        <HelpTip label="ClubLog API key">
          Fetches <b>cty.xml</b>, the DXCC prefix database every account is
          classified against — so it belongs to the server, not to an operator.
          It is <b>not</b> used to download anyone's log; that uses each user's
          own email and app password under <b>My station › ClubLog account</b>.
        </HelpTip>
      </span>
      <input
        type="password"
        bind:value={server.cfg.clublog_api_key}
        placeholder="from clublog.org — one key for the whole server"
      />
    </div>

    <!-- Two shared datasets, three columns: what, when, act. -->
    <table class="refdata">
      <tbody>
        <tr>
          <td class="what">cty.xml<br /><span class="hint">{s?.cty_entities ?? '—'} entities</span></td>
          <td class="when hint">
            {#if server.cfg.read_only.cty_refresh_days}
              auto every {server.cfg.read_only.cty_refresh_days}d ·
            {:else}
              auto off ·
            {/if}
            {#if server.cfg.cty_last_refresh_unix}
              last {ago(server.cfg.cty_last_refresh_unix)} ago
            {:else}
              never downloaded here
            {/if}
          </td>
          <td>
            <button onclick={refreshCty} disabled={busy || !server.cfg.clublog_api_key}>Refresh now</button>
          </td>
        </tr>
        <tr>
          <td class="what">LoTW users<br /><span class="hint">{s?.lotw_users ?? '—'} calls</span></td>
          <td class="when hint">
            {#if server.cfg.read_only.lotw_refresh_days}
              auto every {server.cfg.read_only.lotw_refresh_days}d ·
            {:else}
              auto off ·
            {/if}
            {#if server.cfg.lotw_last_refresh_unix}
              last {ago(server.cfg.lotw_last_refresh_unix)} ago
            {:else}
              never downloaded here
            {/if}
          </td>
          <td><button onclick={refreshLotw} disabled={busy}>Refresh now</button></td>
        </tr>
      </tbody>
    </table>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}

    <ApplySave />

    <p class="hint file-only">
      File-only settings: web {server.cfg.read_only.web_bind} · telnet
      {server.cfg.read_only.telnet_port} · dedupe {server.cfg.read_only.dedupe_window_secs}s ·
      ring {server.cfg.read_only.spot_ring_capacity} · cty refresh
      {server.cfg.read_only.cty_refresh_days}d · LoTW refresh
      {server.cfg.read_only.lotw_refresh_days}d · data dir
      <code>{server.cfg.read_only.data_dir}</code> (edit config/dxca.toml + restart).
    </p>
  </div>
{/if}

<Blacklist />

<style>
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

  .refdata {
    width: auto;
    margin-top: 0.75rem;
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

  p {
    margin: 0.75rem 0 0;
  }
</style>
