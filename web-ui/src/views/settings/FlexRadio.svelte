<script lang="ts">
  // Settings › Server › Destinations › FlexRadio — alerts onto the panadapter.
  //
  // A tab on Destinations rather than its own page: a radio is somewhere
  // spots go, like a UDP feed or an MQTT topic, and it read oddly on its own
  // under My station.
  //
  // That page is admin-only, so this is too. It matches the deployment —
  // admin is the main user and owns the local network; guests log in, set
  // their ClubLog credentials, and choose what their Telegram alerts on.
  // The settings themselves are still per-account, in the operator's own
  // `notify_json`, so nothing had to move server-side.
  //
  // Like the Telegram and Alerts pages it edits ONE shared `notifications`
  // row that the server replaces wholesale, so it loads the WHOLE object and
  // writes it back with only its own fields changed. Never send a partial:
  // it would silently clear whatever the other two own.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  const DEFAULT_PORT = 4992;

  let cfg = $state<any>({
    flex_enabled: false,
    flex_host: '',
    flex_port: DEFAULT_PORT,
    flex_devices: [],
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
      if (!cfg.flex_life_dxcc_minutes) cfg.flex_life_dxcc_minutes = 60;
      if (!cfg.flex_life_band_mode_minutes) cfg.flex_life_band_mode_minutes = 15;
      if (!cfg.flex_life_other_minutes) cfg.flex_life_other_minutes = 1;
      // The server adopts a pre-list account's single radio into the list
      // before it ever reaches here, so this is belt and braces for a row
      // written by something else — and it is what puts an empty account in
      // front of one blank row to type into rather than a bare Add button.
      if (!Array.isArray(cfg.flex_devices) || cfg.flex_devices.length === 0) {
        cfg.flex_devices = cfg.flex_host
          ? [{ host: cfg.flex_host, port: cfg.flex_port || DEFAULT_PORT, enabled: true }]
          : [blank()];
      }
      for (const d of cfg.flex_devices) if (!d.port) d.port = DEFAULT_PORT;
    }
  });

  function blank() {
    return { host: '', port: DEFAULT_PORT, enabled: true };
  }

  function addDevice() {
    cfg.flex_devices = [...cfg.flex_devices, blank()];
  }

  // Removing the last row leaves one blank rather than none: an empty list
  // and a list holding one empty host mean the same thing to the server
  // (both send nowhere), and the blank row is the one that can be typed
  // into without hunting for Add.
  function removeDevice(i: number) {
    const rest = cfg.flex_devices.filter((_: unknown, j: number) => j !== i);
    cfg.flex_devices = rest.length ? rest : [blank()];
  }

  async function save() {
    busy = true; message = ''; error = '';
    // A row whose address was never filled in is not a radio, so it is
    // dropped rather than saved as an entry that can only ever do nothing.
    const devices = cfg.flex_devices
      .filter((d: any) => String(d.host ?? '').trim() !== '')
      .map((d: any) => ({
        host: String(d.host).trim(),
        // `|| DEFAULT_PORT` and not `?? DEFAULT_PORT`: an emptied number
        // field yields '', which is falsy but not nullish, and NaN fails
        // the deserialize.
        port: Number(d.port) || DEFAULT_PORT,
        enabled: d.enabled !== false,
      }));
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      flex_devices: devices,
      // Kept in step with the first radio so a row stays readable by a DXCA
      // that predates the list. The server does this too; sending it here
      // keeps the object we hold identical to the one it stores.
      flex_host: devices[0]?.host ?? '',
      flex_port: devices[0]?.port ?? DEFAULT_PORT,
      flex_life_dxcc_minutes: Number(cfg.flex_life_dxcc_minutes) || 60,
      flex_life_band_mode_minutes: Number(cfg.flex_life_band_mode_minutes) || 15,
      flex_life_other_minutes: Number(cfg.flex_life_other_minutes) || 1,
    });
    busy = false;
    if (r.status === 200) {
      message = devices.length === 1 ? 'Saved.' : `Saved. ${devices.length} radios.`;
      if (devices.length === 0) cfg.flex_devices = [blank()];
    } else error = r.json?.error ?? `HTTP ${r.status}`;
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

  <h3>
    Radios
    <HelpTip label="Radios">
      One row per FlexRadio you want marked. <b>Add as many as you run</b> — a
      second rig on the bench, or another SmartSDR instance — and every alert
      goes to all of them.
      <br /><br />
      The <b>port</b> is SmartSDR's command port, 4992 unless you have changed
      it. A TCP session, not one of the UDP spot outputs. Each radio has its
      own, so two instances on one machine can each take a row.
      <br /><br />
      Clearing a row's <b>On</b> box keeps the address but stops sending to
      it — the way to silence one radio for an evening without retyping its
      IP. A row left with no address is dropped when you save.
    </HelpTip>
  </h3>

  <div class="devices">
    {#each cfg.flex_devices as d, i (i)}
      <div class="device">
        <input class="host" bind:value={d.host} placeholder="192.168.1.148" aria-label="Radio IP" />
        <input
          class="short"
          type="number"
          min="1"
          max="65535"
          bind:value={d.port}
          aria-label="API port"
        />
        <label class="on"><input type="checkbox" bind:checked={d.enabled} />On</label>
        <button
          class="remove"
          onclick={() => removeDevice(i)}
          aria-label="Remove radio {d.host || i + 1}">Remove</button
        >
      </div>
    {/each}
  </div>

  <div class="add">
    <button onclick={addDevice}>Add radio</button>
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

  .devices {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  /* Wraps rather than scrolls: on a phone the four controls stack instead of
     pushing Remove off the edge of the card. */
  .device {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .host {
    flex: 1 1 12rem;
    min-width: 0;
  }

  .on {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    white-space: nowrap;
  }

  .add {
    margin-top: 0.7rem;
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
