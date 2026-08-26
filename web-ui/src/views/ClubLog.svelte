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

<div class="page">
  <div class="card">
    <h2>My ClubLog</h2>
    <p class="dim">
      Your log drives the New DXCC / Slot / Band / Mode highlighting — only
      for your account.
    </p>
    <div class="form-row"><span>Callsign</span><input bind:value={cfg.callsign} /></div>
    <div class="form-row"><span>Email</span><input bind:value={cfg.email} /></div>
    <div class="form-row">
      <span>App password</span>
      <input type="password" bind:value={cfg.app_password} />
    </div>
    <div class="form-row"><span>API key</span><input bind:value={cfg.api_key} /></div>
    <h2>Alert levels</h2>
    <label><input type="checkbox" bind:checked={cfg.alert_new_dxcc} />New DXCC</label>
    <label><input type="checkbox" bind:checked={cfg.alert_new_slot} />New slot (band+mode)</label>
    <label><input type="checkbox" bind:checked={cfg.alert_new_band} />New band</label>
    <label><input type="checkbox" bind:checked={cfg.alert_new_mode} />New mode</label>
    <label>
      <input type="checkbox" bind:checked={cfg.alert_unconfirmed} />
      Treat unconfirmed as not worked (confirmation hunting)
    </label>
    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
      <button onclick={refresh} disabled={busy}>Refresh log now</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
</div>

<style>
  .page { padding: 20px; }
  .actions { display: flex; gap: 10px; margin-top: 14px; }
  label { margin: 4px 0; }
</style>
