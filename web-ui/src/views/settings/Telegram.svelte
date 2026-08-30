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
    // 0 = off, and off is the default. Health alerts belong here rather than
    // on Alerts for the same reason the token does: a threshold is decided
    // once, not retuned while operating.
    notify_feed_quiet_minutes: 0,
    notify_node_down_minutes: 0,
    // The panadapter sink. Independent of telegram_enabled — spots on the
    // radio without a phone buzzing is a reasonable way to run.
    flex_enabled: false,
    flex_host: '',
    flex_port: 4992,
    flex_lifetime_minutes: 20,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/notifications');
    if (r.status === 200 && r.json) {
      cfg = { ...cfg, ...r.json };
      // The server stores 0 to mean "use the default", which is right on the
      // wire and useless to show: a port field reading 0 tells the operator
      // nothing about what will actually be dialled. Fill in the real values
      // for display; 0 still round-trips harmlessly.
      if (!cfg.flex_port) cfg.flex_port = 4992;
      if (!cfg.flex_lifetime_minutes) cfg.flex_lifetime_minutes = 20;
    }
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      cooldown_minutes: Number(cfg.cooldown_minutes) || 15,
      // `|| 0` and not `?? 0`: an emptied number field yields '', which is
      // falsy but not nullish, and NaN would fail the server's deserialize.
      notify_feed_quiet_minutes: Number(cfg.notify_feed_quiet_minutes) || 0,
      notify_node_down_minutes: Number(cfg.notify_node_down_minutes) || 0,
      flex_port: Number(cfg.flex_port) || 4992,
      flex_lifetime_minutes: Number(cfg.flex_lifetime_minutes) || 20,
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

  <h3>
    Health alerts
    <HelpTip label="Health alerts">
      Ping me when DXCA is running but <em>nothing is reaching it</em> — the
      failure that otherwise goes unnoticed for weeks, because the web GUI
      answers perfectly while the feed is dead.
      <br /><br />
      <b>It cannot tell you this host has died.</b> Nothing running here could,
      and Telegram needs the internet, so a connectivity failure silences the
      alert about the connectivity failure. Covering that needs a watcher
      somewhere else.
      <br /><br />
      Both are <b>0 = off</b>. You get one message when a condition starts and
      one when it clears, never a repeat in between.
    </HelpTip>
  </h3>

  <div class="settings-form">
    <span class="label">
      No spots (min)
      <HelpTip label="No spots">
        Nothing has arrived from <em>any</em> source — decoders and cluster
        nodes alike. Usually means the decoders were closed or the radio is
        off. Try 30 minutes on a busy station; a quiet one wants longer.
      </HelpTip>
    </span>
    <input
      class="short"
      type="number"
      min="0"
      max="1440"
      bind:value={cfg.notify_feed_quiet_minutes}
    />
    <span class="label">
      Node down (min)
      <HelpTip label="Node down">
        One cluster node has been <em>disconnected</em> this long while the
        rest carry on. Keyed on the connection, not on traffic — a node
        sitting connected with no spots is normal for hours, so alerting on
        silence would cry wolf every quiet afternoon.
      </HelpTip>
    </span>
    <input
      class="short"
      type="number"
      min="0"
      max="1440"
      bind:value={cfg.notify_node_down_minutes}
    />
  </div>

  <h3>
    FlexRadio panadapter
    <HelpTip label="FlexRadio panadapter">
      Put these same alerts on the radio's panadapter, colour-coded by level
      — red for New DXCC, blue New Band, amber New Mode, orange New Slot, and
      the four <b>?</b> levels in the same hues, dimmed. The colours match the
      Spots screen, so a red mark means on the radio what it means here.
      <br /><br />
      Everything that narrows Telegram narrows this too: levels, bands, modes,
      spotter kind, band mask and the cooldown. One set of choices, two places
      to see the result.
      <br /><br />
      <b>Only alerts go to the radio, never the whole feed.</b> If something
      else is already feeding it every cluster spot, disconnect that first or
      each alert will arrive twice.
    </HelpTip>
  </h3>

  <label class="enable">
    <input type="checkbox" bind:checked={cfg.flex_enabled} />Send alerts to a FlexRadio
  </label>

  <div class="settings-form">
    <span class="label">Radio IP</span>
    <input bind:value={cfg.flex_host} placeholder="192.168.1.148" />
    <span class="label">
      API port
      <HelpTip label="API port">
        SmartSDR's command port, 4992 unless you have changed it. This is a
        TCP session, not one of the UDP broadcast destinations.
      </HelpTip>
    </span>
    <input class="short" type="number" min="1" max="65535" bind:value={cfg.flex_port} />
    <span class="label">
      Spot life (min)
      <HelpTip label="Spot life">
        How long each spot stays on the panadapter before the radio drops it.
      </HelpTip>
    </span>
    <input
      class="short"
      type="number"
      min="1"
      max="240"
      bind:value={cfg.flex_lifetime_minutes}
    />
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

  h3 {
    margin: 1.4rem 0 0.7rem;
    font-size: 0.95rem;
    color: var(--muted);
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
