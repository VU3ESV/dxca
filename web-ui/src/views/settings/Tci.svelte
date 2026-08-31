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

  let cfg = $state<any>({
    tci_enabled: false,
    tci_host: '',
    tci_port: 40001,
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
      if (!cfg.tci_port) cfg.tci_port = 40001;
      if (!cfg.tci_life_dxcc_minutes) cfg.tci_life_dxcc_minutes = 60;
      if (!cfg.tci_life_band_mode_minutes) cfg.tci_life_band_mode_minutes = 15;
      if (!cfg.tci_life_other_minutes) cfg.tci_life_other_minutes = 1;
    }
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      // `|| default` and not `?? default`: an emptied number field yields
      // '', which is falsy but not nullish, and NaN fails the deserialize.
      tci_port: Number(cfg.tci_port) || 40001,
      tci_life_dxcc_minutes: Number(cfg.tci_life_dxcc_minutes) || 60,
      tci_life_band_mode_minutes: Number(cfg.tci_life_band_mode_minutes) || 15,
      tci_life_other_minutes: Number(cfg.tci_life_other_minutes) || 1,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
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

  <div class="settings-form">
    <span class="label">Radio IP</span>
    <input bind:value={cfg.tci_host} placeholder="192.168.1.60" />
    <span class="label">
      TCI port
      <HelpTip label="TCI port">
        ExpertSDR3's TCI server port, 40001 unless you have changed it in
        <em>Options › TCI</em>. A WebSocket, not one of the UDP spot outputs —
        the same address the skimmers and loggers connect to.
      </HelpTip>
    </span>
    <input class="short" type="number" min="1" max="65535" bind:value={cfg.tci_port} />
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
      stays until you clear it in ExpertSDR3.
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
