<script lang="ts">
  // My alerts (plan §8 page 5): Telegram setup + test, cooldown, levels.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';

  let cfg = $state<any>({
    telegram_enabled: false, telegram_bot_token: '', telegram_chat_id: '',
    cooldown_minutes: 15, notify_new_dxcc: true, notify_new_slot: true,
    notify_new_band: true, notify_new_mode: true,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/notifications');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      cooldown_minutes: Number(cfg.cooldown_minutes) || 15,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  async function test() {
    busy = true; message = ''; error = '';
    const r = await api('POST', '/api/telegram/test');
    busy = false;
    if (r.status === 200) message = 'Test message sent — check Telegram.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }
</script>

<div class="page narrow">
  <div class="card">
    <h2>My alerts — Telegram</h2>
    <label class="enable">
      <input type="checkbox" bind:checked={cfg.telegram_enabled} />Enable Telegram alerts
    </label>
    <div class="settings-form">
      <span class="label">Bot token</span>
      <input type="password" bind:value={cfg.telegram_bot_token} placeholder="from @BotFather" />
      <span class="label">Chat ID</span>
      <input bind:value={cfg.telegram_chat_id} />
      <span class="label">Cooldown (min)</span>
      <input class="short" type="number" min="5" max="60" bind:value={cfg.cooldown_minutes} />
    </div>

    <h2>Notify on</h2>
    <div class="check-list">
      <label><input type="checkbox" bind:checked={cfg.notify_new_dxcc} />New DXCC</label>
      <label><input type="checkbox" bind:checked={cfg.notify_new_slot} />New slot</label>
      <label><input type="checkbox" bind:checked={cfg.notify_new_band} />New band</label>
      <label><input type="checkbox" bind:checked={cfg.notify_new_mode} />New mode</label>
    </div>

    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
      <button onclick={test} disabled={busy}>Send test message</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
</div>

<style>
  /* The master switch stands ahead of the fields it governs, so it sits above
     the label grid rather than inside it. */
  .enable {
    margin-bottom: 0.9rem;
  }

  /* A cooldown is two digits — a full-width field would promise otherwise. */
  .short {
    width: 6rem;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
