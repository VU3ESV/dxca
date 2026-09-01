<script lang="ts">
  // Settings › Server › Destinations › TCI — alerts onto an ExpertSDR3 panorama.
  //
  // The FlexRadio tab's sibling, and deliberately its near-twin: the two are
  // the same feature aimed at different radios, so anything that reads
  // differently here is a difference the operator has to learn for no reason.
  //
  // The one real divergence is the lifetimes. SmartSDR is told how long to
  // keep a spot and does it; TCI has no such field, so DXCA holds the
  // deadline and sends SPOT_DELETE itself — which means a restart leaves
  // whatever is on the panorama there. That is worth saying on the page,
  // because the operator is the one who will see it.
  //
  // Like Telegram, Alerts and FlexRadio it edits ONE shared `notifications`
  // row that the server replaces wholesale, so it loads the WHOLE object and
  // writes it back with only its own fields changed. Never send a partial:
  // it would silently clear whatever the others own.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  const DEFAULT_PORT = 40001;

  let cfg = $state<any>({
    tci_enabled: false,
    tci_host: '',
    tci_port: DEFAULT_PORT,
    tci_devices: [],
    tci_life_dxcc_minutes: 60,
    tci_life_band_mode_minutes: 15,
    tci_life_other_minutes: 1,
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
      if (!cfg.tci_life_dxcc_minutes) cfg.tci_life_dxcc_minutes = 60;
      if (!cfg.tci_life_band_mode_minutes) cfg.tci_life_band_mode_minutes = 15;
      if (!cfg.tci_life_other_minutes) cfg.tci_life_other_minutes = 1;
      // The server adopts a pre-list account's single radio into the list
      // before it ever reaches here, so this is belt and braces for a row
      // written by something else — and it is what puts an empty account in
      // front of one blank row to type into rather than a bare Add button.
      if (!Array.isArray(cfg.tci_devices) || cfg.tci_devices.length === 0) {
        cfg.tci_devices = cfg.tci_host
          ? [{ host: cfg.tci_host, port: cfg.tci_port || DEFAULT_PORT, enabled: true }]
          : [blank()];
      }
      for (const d of cfg.tci_devices) if (!d.port) d.port = DEFAULT_PORT;
    }
  });

  function blank() {
    return { host: '', port: DEFAULT_PORT, enabled: true };
  }

  function addDevice() {
    cfg.tci_devices = [...cfg.tci_devices, blank()];
  }

  // Removing the last row leaves one blank rather than none: an empty list
  // and a list holding one empty host mean the same thing to the server
  // (both send nowhere), and the blank row is the one that can be typed
  // into without hunting for Add.
  function removeDevice(i: number) {
    const rest = cfg.tci_devices.filter((_: unknown, j: number) => j !== i);
    cfg.tci_devices = rest.length ? rest : [blank()];
  }

  async function save() {
    busy = true; message = ''; error = '';
    // A row whose address was never filled in is not a radio, so it is
    // dropped rather than saved as an entry that can only ever do nothing.
    const devices = cfg.tci_devices
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
      tci_devices: devices,
      // Kept in step with the first radio so a row stays readable by a DXCA
      // that predates the list. The server does this too; sending it here
      // keeps the object we hold identical to the one it stores.
      tci_host: devices[0]?.host ?? '',
      tci_port: devices[0]?.port ?? DEFAULT_PORT,
      tci_life_dxcc_minutes: Number(cfg.tci_life_dxcc_minutes) || 60,
      tci_life_band_mode_minutes: Number(cfg.tci_life_band_mode_minutes) || 15,
      tci_life_other_minutes: Number(cfg.tci_life_other_minutes) || 1,
    });
    busy = false;
    if (r.status === 200) {
      message = devices.length === 1 ? 'Saved.' : `Saved. ${devices.length} radios.`;
      if (devices.length === 0) cfg.tci_devices = [blank()];
    } else error = r.json?.error ?? `HTTP ${r.status}`;
  }
</script>

<div class="card">
  <h2>
    ExpertSDR3 panorama (TCI)
    <HelpTip label="ExpertSDR3 panorama (TCI)">
      Puts <em>your alerts</em> on an ExpertSDR3 panorama over the TCI
      protocol, colour-coded by level — the same colours the Spots screen and
      the FlexRadio tab use, so a red mark means the same thing everywhere.
      <br /><br />
      For SunSDR and the other radios ExpertSDR3 drives. <b>ExpertSDR3 itself
      is the server</b>, so it has to be running, and its TCI server switched
      on, before anything arrives.
      <br /><br />
      <b>Only alerts are sent, never the whole feed</b>, and that is the
      point: the alert level comes from your ClubLog log, which nothing else
      on the network can see. Everything that narrows Telegram narrows this
      too — levels, bands, modes, spotter kind, band mask, cooldown — and it
      works whether or not Telegram itself is switched on.
      <br /><br />
      <b>If something else already feeds the panorama every cluster spot,
      disconnect it first</b>, or each alert will arrive twice.
      <br /><br />
      DXCA cannot take over your radio: the only commands it ever sends are a
      spot and the deletion of a spot it placed itself. It never clears the
      panorama, so spots put there by another logger are left alone.
    </HelpTip>
  </h2>

  <label class="enable">
    <input type="checkbox" bind:checked={cfg.tci_enabled} />Send alerts to ExpertSDR3
  </label>

  <h3>
    Radios
    <HelpTip label="Radios">
      One row per ExpertSDR3 you want marked. <b>Add as many as you run</b> —
      a second rig on the bench, or another ExpertSDR3 instance driving its
      own panorama — and every alert goes to all of them.
      <br /><br />
      The <b>port</b> is ExpertSDR3's TCI server port, 40001 unless you have
      changed it in <em>Options › TCI</em>. A WebSocket, not one of the UDP
      spot outputs — the same address the skimmers and loggers connect to.
      Each radio has its own, so two ExpertSDR3 instances on one machine can
      each take a row.
      <br /><br />
      Clearing a row's <b>On</b> box keeps the address but stops sending to
      it — the way to silence one radio for an evening without retyping its
      IP. A row left with no address is dropped when you save.
    </HelpTip>
  </h3>

  <div class="devices">
    {#each cfg.tci_devices as d, i (i)}
      <div class="device">
        <input class="host" bind:value={d.host} placeholder="192.168.1.60" aria-label="Radio IP" />
        <input
          class="short"
          type="number"
          min="1"
          max="65535"
          bind:value={d.port}
          aria-label="TCI port"
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
      <br /><br />
      <b>TCI has no expiry of its own</b>, so DXCA removes each spot when its
      time is up. If DXCA restarts in between, whatever is on the panorama
      stays until you clear it in ExpertSDR3 — a dropped connection is
      different, and the pending deletions go out when it reconnects.
    </HelpTip>
  </h3>

  <div class="settings-form">
    <span class="label">New DXCC (min)</span>
    <input class="short" type="number" min="1" max="1440" bind:value={cfg.tci_life_dxcc_minutes} />
    <span class="label">Band/Mode (min)</span>
    <input
      class="short"
      type="number"
      min="1"
      max="1440"
      bind:value={cfg.tci_life_band_mode_minutes}
    />
    <span class="label">
      Others (min)
      <HelpTip label="Others">
        New Slot, and the four worked-but-unconfirmed levels — <b>?</b> DXCC,
        Band, Mode and Slot. Most of the alert traffic; keep it short.
      </HelpTip>
    </span>
    <input class="short" type="number" min="1" max="1440" bind:value={cfg.tci_life_other_minutes} />
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
