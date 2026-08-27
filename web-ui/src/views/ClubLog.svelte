<script lang="ts">
  // My ClubLog (plan §8 page 4): credentials, alert toggles, refresh.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';

  let cfg = $state<any>({
    callsign: '', email: '', app_password: '', api_key: '',
    alert_new_dxcc: true, alert_new_slot: true, alert_new_band: true,
    alert_new_mode: true, alert_unconfirmed: false,
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
    const r = await api('PUT', '/api/config/me/clublog', cfg);
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  async function refresh() {
    busy = true; message = 'Downloading log — this can take a minute…'; error = '';
    const r = await api('POST', '/api/clublog/refresh');
    busy = false;
    if (r.status === 200) {
      message = `Refreshed: ${r.json.qso_count} QSOs, ${r.json.dxcc_count} DXCC entities.`;
    } else {
      message = '';
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }
</script>

<div class="page narrow">
  <div class="card">
    <h2>My ClubLog</h2>
    <p class="hint intro">
      Your log drives the New DXCC / Slot / Band / Mode highlighting — only
      for your account.
    </p>
    <div class="settings-form">
      <span class="label">Callsign</span>
      <input bind:value={cfg.callsign} autocapitalize="characters" />
      <span class="label">Email</span>
      <input bind:value={cfg.email} />
      <span class="label">App password</span>
      <input type="password" bind:value={cfg.app_password} />
      <span class="label">API key</span>
      <input bind:value={cfg.api_key} />
    </div>

    <h2>Alert levels</h2>
    <div class="check-list">
      <label><input type="checkbox" bind:checked={cfg.alert_new_dxcc} />New DXCC</label>
      <label><input type="checkbox" bind:checked={cfg.alert_new_slot} />New slot (band+mode)</label>
      <label><input type="checkbox" bind:checked={cfg.alert_new_band} />New band</label>
      <label><input type="checkbox" bind:checked={cfg.alert_new_mode} />New mode</label>
      <label>
        <input type="checkbox" bind:checked={cfg.alert_unconfirmed} />
        Treat unconfirmed as not worked (confirmation hunting)
      </label>
    </div>

    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
      <button onclick={refresh} disabled={busy}>Refresh log now</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
</div>

<style>
  /* Sits between the card title and the fields it explains, so it takes the
     gap rather than adding one. */
  .intro {
    margin: -0.35rem 0 1rem;
    line-height: 1.5;
    max-width: 34rem;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
