<script lang="ts">
  // Settings › My station › FlexRadio — alerts onto the panadapter.
  //
  // Under **My station**, not Server, and that is deliberate: the Server
  // group is admin-only, while this is per-account config living in the
  // operator's own `notify_json`. Filed beside the spot outputs it would be
  // invisible to any non-admin whose radio it actually is.
  //
  // Like the Telegram and Alerts pages it edits ONE shared `notifications`
  // row that the server replaces wholesale, so it loads the WHOLE object and
  // writes it back with only its own fields changed. Never send a partial:
  // it would silently clear whatever the other two own.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  let cfg = $state<any>({
    flex_enabled: false,
    flex_host: '',
    flex_port: 4992,
    flex_life_dxcc_minutes: 60,
    flex_life_band_mode_minutes: 15,
    flex_life_other_minutes: 1,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/notifications');
    if (r.status === 200 && r.json) {
      cfg = { ...cfg, ...r.json };
      // The server stores 0 to mean "use the default", which is right on the
      // wire and useless to show: a lifetime field reading 0 tells the
      // operator nothing about how long a spot will actually last. Fill the
      // real values in for display; 0 still round-trips harmlessly.
      if (!cfg.flex_port) cfg.flex_port = 4992;
      if (!cfg.flex_life_dxcc_minutes) cfg.flex_life_dxcc_minutes = 60;
      if (!cfg.flex_life_band_mode_minutes) cfg.flex_life_band_mode_minutes = 15;
      if (!cfg.flex_life_other_minutes) cfg.flex_life_other_minutes = 1;
    }
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      // `|| default` and not `?? default`: an emptied number field yields
      // '', which is falsy but not nullish, and NaN fails the deserialize.
      flex_port: Number(cfg.flex_port) || 4992,
      flex_life_dxcc_minutes: Number(cfg.flex_life_dxcc_minutes) || 60,
      flex_life_band_mode_minutes: Number(cfg.flex_life_band_mode_minutes) || 15,
      flex_life_other_minutes: Number(cfg.flex_life_other_minutes) || 1,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }
</script>

<div class="card">
  <h2>
    FlexRadio panadapter
    <HelpTip label="FlexRadio panadapter">
      Puts <em>your alerts</em> on the radio's panadapter over the SmartSDR
      API, colour-coded by level — the same colours the Spots screen uses, so
      a red mark means on the radio what a red row means here.
      <br /><br />
      <b>Only alerts are sent, never the whole feed</b>, and that is the
      point: the alert level comes from your ClubLog log, which nothing else
      on the network can see. Everything that narrows Telegram narrows this
      too — levels, bands, modes, spotter kind, band mask, cooldown — and it
      works whether or not Telegram itself is switched on.
      <br /><br />
      <b>If something else already feeds the radio every cluster spot,
      disconnect it first</b>, or each alert will arrive twice.
      <br /><br />
      DXCA cannot take over your radio: it never claims a station or a slice,
      and the only thing it ever sends is a spot.
    </HelpTip>
  </h2>

  <label class="enable">
    <input type="checkbox" bind:checked={cfg.flex_enabled} />Send alerts to a FlexRadio
  </label>

  <div class="settings-form">
    <span class="label">Radio IP</span>
    <input bind:value={cfg.flex_host} placeholder="192.168.1.148" />
    <span class="label">
      API port
      <HelpTip label="API port">
        SmartSDR's command port, 4992 unless you have changed it. A TCP
        session, not one of the UDP spot outputs.
      </HelpTip>
    </span>
    <input class="short" type="number" min="1" max="65535" bind:value={cfg.flex_port} />
  </div>

  <h3>
    How long spots stay
    <HelpTip label="How long spots stay">
      The ladder matters more than the numbers. A <b>New DXCC</b> is worth
      leaving up for an hour — you may be mid-QSO when it appears and still
      want to find it afterwards. A <b>New Band or Mode</b> is worth about as
      long as you would stay on a band looking for it.
      <br /><br />
      <b>Keep the last one short.</b> New Slot and the four <b>?</b> levels
      are most of the alert traffic, and giving them twenty minutes will paint
      the whole band inside an hour — burying the one red mark this is for.
    </HelpTip>
  </h3>

  <div class="settings-form">
    <span class="label">New DXCC (min)</span>
    <input class="short" type="number" min="1" max="1440" bind:value={cfg.flex_life_dxcc_minutes} />
    <span class="label">Band/Mode (min)</span>
    <input
      class="short"
      type="number"
      min="1"
      max="1440"
      bind:value={cfg.flex_life_band_mode_minutes}
    />
    <span class="label">
      Others (min)
      <HelpTip label="Others">
        New Slot, and the four worked-but-unconfirmed levels — <b>?</b> DXCC,
        Band, Mode and Slot. Most of the alert traffic; keep it short.
      </HelpTip>
    </span>
    <input
      class="short"
      type="number"
      min="1"
      max="1440"
      bind:value={cfg.flex_life_other_minutes}
    />
  </div>

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
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
