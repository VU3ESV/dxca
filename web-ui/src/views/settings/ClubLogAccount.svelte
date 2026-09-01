<script lang="ts">
  // Settings › My station › ClubLog account — the credentials that fetch
  // this account's log, ClubLog and LoTW both.
  //
  // The alert ladder lived on this page from 2026-08-29 to 2026-09-01 (the
  // classifier gate reads the log these credentials fetch, so the pairing
  // was honest). It moved to its own **Awards** page when the chaseable
  // awards arrived, on Manoj's direction: which awards you chase is an
  // awards question, and fourteen checkboxes under "ClubLog account" buried
  // it. This page is credentials again; the ladder's two-control story
  // (classifier here-ish vs Telegram narrowing on Alerts) is told on Awards.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  // 0 = manual only. Mirrors the server's `refresh_hours`, whose own default
  // is 24 — a log that only moves when someone presses a button means
  // today's QSOs keep alerting as New DXCC tomorrow.
  const INTERVALS: [number, string][] = [
    [0, 'Manual only'],
    [6, 'Every 6 hours'],
    [12, 'Every 12 hours'],
    [24, 'Daily'],
    [48, 'Every 2 days'],
    [168, 'Weekly'],
  ];

  // The level flags stay in this object (one stored row, edited by two
  // pages the way Alerts and Telegram share notify_json): loaded whole,
  // written back whole with only this page's fields touched. The defaults
  // below cover a failed load so a save can never blank the ladder.
  let cfg = $state<any>({
    callsign: '', email: '', app_password: '', refresh_hours: 24,
    lotw_login: '', lotw_password: '',
    alert_new_dxcc: true, alert_new_band: true, alert_new_mode: true, alert_new_slot: true,
    alert_unconf_dxcc: false, alert_unconf_band: false,
    alert_unconf_mode: false, alert_unconf_slot: false,
    alert_new_iota: false, alert_new_state: false, alert_new_grid: false,
    alert_unconf_iota: false, alert_unconf_state: false, alert_unconf_grid: false,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
  });

  async function save() {
    busy = true; message = ''; error = '';
    // The <select> yields strings; the server's refresh_hours is an integer and
    // a quoted "24" fails its deserialize. Coerced on save, not on bind, so the
    // select still matches the loaded value by identity.
    const r = await api('PUT', '/api/config/me/clublog', {
      ...cfg,
      refresh_hours: Number(cfg.refresh_hours) || 0,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  async function refresh() {
    busy = true; message = 'Downloading log — this can take a minute…'; error = '';
    const r = await api('POST', '/api/clublog/refresh');
    busy = false;
    if (r.status === 200) {
      message = `Refreshed: ${r.json.qso_count} QSOs, ${r.json.dxcc_count} DXCC entities. See Stats › ClubLog.`;
    } else {
      message = '';
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }
</script>

<div class="card">
  <h2>
    ClubLog account
    <HelpTip label="ClubLog account">
      Your log drives the New / ? highlighting — only for your account. The
      <b>ClubLog API key</b> is not here: it only fetches the shared DXCC prefix
      database, so it is one server-wide setting under <b>Server › Reference
      data</b>. These credentials download <em>your</em> log.
    </HelpTip>
  </h2>
  <div class="settings-form">
    <span class="label">Callsign</span>
    <input bind:value={cfg.callsign} autocapitalize="characters" />
    <span class="label">Email</span>
    <input bind:value={cfg.email} />
    <span class="label">App password</span>
    <input type="password" bind:value={cfg.app_password} />
    <span class="label">
      Auto-refresh
      <HelpTip label="Auto-refresh">
        {#if cfg.refresh_hours > 0}
          Re-downloads your log in the background, so a QSO worked today stops
          showing as New DXCC tomorrow. The button below still works any time.
        {:else}
          Your log will only change when you press <b>Refresh log now</b> —
          anything you work keeps alerting as new until you do.
        {/if}
      </HelpTip>
    </span>
    <select bind:value={cfg.refresh_hours}>
      {#each INTERVALS as [hours, label] (hours)}
        <option value={hours}>{label}</option>
      {/each}
    </select>
    <span class="label">
      LoTW username
      <HelpTip label="LoTW username">
        <span class="para">
          Optional, for the awards on <b>Settings › My station › Awards</b>:
          your LoTW QSL report is the one source that says which state, grid
          and island your confirmations came from — ClubLog's export cannot
          carry those fields.
        </span>
        <span class="para">
          Your LoTW <b>website</b> login, sent only to lotw.arrl.org, stored
          like the ClubLog credentials above (README §Secrets). Leave both
          blank and the chased awards track worked-only.
        </span>
      </HelpTip>
    </span>
    <input bind:value={cfg.lotw_login} autocapitalize="characters" />
    <span class="label">LoTW password</span>
    <input type="password" bind:value={cfg.lotw_password} />
  </div>

  <!-- The alert ladder is on Settings › Awards; a one-line pointer beats a
       page of checkboxes here. -->
  <p class="hint">
    What this log is allowed to flag — the alert ladder and the awards you
    chase — lives under <b>Awards</b> in this rail.
  </p>

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
    <button onclick={refresh} disabled={busy}>Refresh log now</button>
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  p {
    margin: 0.75rem 0 0;
  }
</style>
