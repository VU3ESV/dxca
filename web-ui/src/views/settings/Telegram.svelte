<script lang="ts">
  // Settings › My station › Telegram — the credentials and the cooldown.
  //
  // The NARROWING half of the old My Alerts card stays on the Alerts tab:
  // which levels ping, on which bands and modes, skimmers, band mask. Those
  // are retuned while operating, the same argument that keeps the Spots
  // filters on Spots. A bot token is not — it is typed once and never again.
  //
  // Both halves live in one `notifications` row that the server replaces
  // wholesale, so this page loads the WHOLE object and writes it back with
  // only its own fields changed. Never send a partial: it would silently clear
  // the narrowing the Alerts tab owns.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  let cfg = $state<any>({
    telegram_enabled: false,
    telegram_bot_token: '',
    telegram_chat_id: '',
    cooldown_minutes: 15,
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

<div class="card">
  <h2>
    Telegram
    <HelpTip label="Telegram">
      The bot that carries your alerts. What it pings you <em>for</em> — which
      levels, which bands and modes, whether skimmers count — lives on the
      <b>Alerts</b> tab, because that is retuned while you operate. This page is
      the part you set once.
    </HelpTip>
  </h2>

  <label class="enable">
    <input type="checkbox" bind:checked={cfg.telegram_enabled} />Enable Telegram alerts
  </label>

  <div class="settings-form">
    <span class="label">Bot token</span>
    <input type="password" bind:value={cfg.telegram_bot_token} placeholder="from @BotFather" />
    <span class="label">Chat ID</span>
    <input bind:value={cfg.telegram_chat_id} />
    <span class="label">
      Cooldown (min)
      <HelpTip label="Cooldown">
        The least time between two pings about the same station, so a call that
        is being spotted every thirty seconds does not empty your battery.
      </HelpTip>
    </span>
    <input class="short" type="number" min="5" max="60" bind:value={cfg.cooldown_minutes} />
  </div>

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
    <button onclick={test} disabled={busy}>Send test message</button>
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  .enable {
    margin-bottom: 0.9rem;
  }

  .short {
    width: 6rem;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
