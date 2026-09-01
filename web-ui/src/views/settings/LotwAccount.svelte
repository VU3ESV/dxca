<script lang="ts">
  // Settings › My station › LoTW account — the ARRL login that fetches this
  // account's QSL report.
  //
  // Its own page from 2026-09-01 (Manoj: "my clublog and my lotw to be
  // different tabs"). They ARE two accounts at two organisations, and the
  // two fields were easy to miss as a footnote under the ClubLog form.
  //
  // Both pages still edit ONE stored row (`clublog_json`, the server's
  // ClubLogUserConfig) — the same arrangement Alerts and Telegram have over
  // `notify_json`. That is why each loads the WHOLE object and writes it
  // back whole with only its own fields changed: a partial PUT would let
  // serde's defaults blank the other page's credentials.
  //
  // Deliberately NOT its own storage struct. Splitting a settings page is a
  // UI change; making it a schema change is how a menu tidy turns into a
  // migration (see docs/MULTI-STATION.md for the time that went wrong).
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  // Defaults cover a failed load, so a save can never blank what this page
  // does not show.
  let cfg = $state<any>({
    callsign: '', email: '', app_password: '', refresh_hours: 24,
    lotw_login: '', lotw_password: '',
    alert_new_dxcc: true, alert_new_band: true, alert_new_mode: true, alert_new_slot: true,
    alert_unconf_dxcc: false, alert_unconf_band: false,
    alert_unconf_mode: false, alert_unconf_slot: false,
    alert_new_iota: false, alert_new_state: false, alert_new_grid: false,
    alert_unconf_iota: false, alert_unconf_state: false, alert_unconf_grid: false,
  });
  let loaded = $state(false);
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
    loaded = true;
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/clublog', {
      ...cfg,
      refresh_hours: Number(cfg.refresh_hours) || 0,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  let configured = $derived(loaded && cfg.lotw_login && cfg.lotw_password);
</script>

<div class="card">
  <h2>
    LoTW
    <HelpTip label="LoTW">
      <span class="para">
        Your <b>Logbook of the World</b> website login — the same one you use
        at lotw.arrl.org, not a TQSL certificate and not your ClubLog
        password. Sent only to lotw.arrl.org, and stored like every other
        credential here (README §Secrets).
      </span>
      <span class="para">
        <b>What it is for:</b> your <b>QSL report</b>, the list of contacts
        LoTW has confirmed. It is the only source that says which
        <b>state</b>, <b>grid square</b> and <b>island</b> a confirmation
        came from — ClubLog's export carries none of those — so it fills in
        the <em>confirmed</em> half of WAS, VUCC and IOTA under
        <b>Awards</b>. DXCC confirmations keep coming from ClubLog either
        way.
      </span>
      <span class="para">
        Optional. Leave it blank and everything else still works; the awards
        simply track worked-only.
      </span>
    </HelpTip>
  </h2>

  <div class="settings-form">
    <span class="label">LoTW username</span>
    <input bind:value={cfg.lotw_login} autocapitalize="characters" />
    <span class="label">LoTW password</span>
    <input type="password" bind:value={cfg.lotw_password} />
  </div>

  <!-- No "download now" button on purpose: the report is fetched as part of
       a ClubLog log refresh (the matrix is rebuilt from scratch each time, so
       the report has to be merged on every rebuild) and cached for a week in
       between. A separate button here would imply a separate schedule that
       does not exist. -->
  <p class="hint">
    {#if configured}
      The report is downloaded with your next <b>log refresh</b> (ClubLog
      account › Refresh log now) and re-used for a week after that.
    {:else if loaded}
      Not set — WAS, VUCC and IOTA will show worked totals but no confirmed
      ones.
    {/if}
  </p>

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  p {
    margin: 0.75rem 0 0;
  }
</style>
