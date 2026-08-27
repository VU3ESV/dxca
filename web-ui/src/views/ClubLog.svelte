<script lang="ts">
  // My ClubLog (plan §8 page 4): credentials, alert toggles, refresh.
  import { api, ago } from '../lib/api';
  import { onMount } from 'svelte';
  import { loadReference, levels } from '../lib/reference.svelte';

  // Level key → the classifier toggle that decides whether a spot is ever
  // flagged as that level at all. This is the widest of the three controls:
  // switch a level off here and it disappears from the feed AND from
  // Telegram, because the classifier never assigns it.
  // The <select> yields strings; the server's refresh_hours is an integer and
  // a quoted "24" fails its deserialize. Coerced on save, not on bind, so the
  // select still matches the loaded value by identity.
  const FIELD: Record<string, string> = {
    newDXCC: 'alert_new_dxcc',
    newBand: 'alert_new_band',
    newMode: 'alert_new_mode',
    newSlot: 'alert_new_slot',
    unconfDXCC: 'alert_unconf_dxcc',
    unconfBand: 'alert_unconf_band',
    unconfMode: 'alert_unconf_mode',
    unconfSlot: 'alert_unconf_slot',
  };

  // 0 = manual only. Mirrors the server's `refresh_hours`, whose own default
  // is 24 — a log that only moves when someone presses a button means
  // today's QSOs keep alerting as New DXCC tomorrow.
  const INTERVALS: [number, string][] = [
    [0, 'Manual only'],
    [6, 'Every 6 hours'],
    [12, 'Every 12 hours'],
    [24, 'Daily'],
    [48, 'Every 2 days'],
    [168, 'Weekly'],
  ];

  let cfg = $state<any>({
    callsign: '', email: '', app_password: '',
    refresh_hours: 24,
    alert_new_dxcc: true, alert_new_slot: true, alert_new_band: true,
    alert_new_mode: true,
    alert_unconf_dxcc: false, alert_unconf_slot: false,
    alert_unconf_band: false, alert_unconf_mode: false,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  // The statistics come from the same endpoint the Spots station card uses,
  // so the two can never disagree about what the log holds.
  let station = $state<any>(null);
  async function loadStation() {
    const r = await api('GET', '/api/me/station');
    if (r.status === 200) station = r.json;
  }

  onMount(async () => {
    await loadReference();
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
    await loadStation();
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

  async function refresh() {
    busy = true; message = 'Downloading log — this can take a minute…'; error = '';
    const r = await api('POST', '/api/clublog/refresh');
    busy = false;
    if (r.status === 200) {
      message = `Refreshed: ${r.json.qso_count} QSOs, ${r.json.dxcc_count} DXCC entities.`;
      // The whole point of the card: a refresh must be visible as numbers,
      // not just as a sentence that scrolls away.
      await loadStation();
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
      Your log drives the New / ? highlighting — only for your account.
      The <b>ClubLog API key</b> is not here: it only fetches the shared DXCC
      prefix database, so it is one server-wide setting an admin manages in
      <b>System</b>. These credentials download <em>your</em> log.
    </p>
    <div class="settings-form">
      <span class="label">Callsign</span>
      <input bind:value={cfg.callsign} autocapitalize="characters" />
      <span class="label">Email</span>
      <input bind:value={cfg.email} />
      <span class="label">App password</span>
      <input type="password" bind:value={cfg.app_password} />
      <span class="label">Auto-refresh</span>
      <select bind:value={cfg.refresh_hours}>
        {#each INTERVALS as [hours, label] (hours)}
          <option value={hours}>{label}</option>
        {/each}
      </select>
    </div>
    <p class="hint note">
      {#if cfg.refresh_hours > 0}
        Re-downloads your log in the background, so a QSO worked today stops
        showing as New DXCC tomorrow. The button below still works any time.
      {:else}
        Your log will only change when you press <b>Refresh log now</b> —
        anything you work keeps alerting as new until you do.
      {/if}
    </p>

    <h2>Alert levels</h2>
    <p class="hint sub">
      Which levels this account flags at all. <b>New</b> means never worked;
      <b>?</b> means worked and still not confirmed — the QSL gap you close by
      working it again. A level switched off here never reaches the spots feed
      or Telegram.
    </p>
    <div class="levels">
      {#each levels() as l (l.key)}
        <label data-level={l.key}>
          <input type="checkbox" bind:checked={cfg[FIELD[l.key]]} />
          <span class="level-dot"></span>{l.label}
        </label>
      {/each}
    </div>

    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
      <button onclick={refresh} disabled={busy}>Refresh log now</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>

  <!-- What the log actually contains. Every number is entities, not QSOs,
       because that is what the awards count — and worked sits beside
       confirmed throughout, since the gap between them IS the QSL chase. -->
  {#if station?.stats}
    <div class="card">
      <h2>Log statistics</h2>
      <p class="hint sub">
        {station.qso_count ?? 0} QSOs
        {#if station.log_callsign}for <b>{station.log_callsign}</b>{/if}
        {#if station.last_refresh_unix}· refreshed {ago(station.last_refresh_unix)} ago{/if}
      </p>

      <dl class="stats">
        <div><dt>DXCC worked</dt><dd class="num">{station.stats.dxcc_worked}</dd></div>
        <div><dt>DXCC confirmed</dt><dd class="num ok-num">{station.stats.dxcc_confirmed}</dd></div>
        <div><dt>Challenge worked</dt><dd class="num">{station.stats.challenge_worked}</dd></div>
        <div><dt>Challenge confirmed</dt><dd class="num ok-num">{station.stats.challenge_confirmed}</dd></div>
        <div><dt>Slots worked</dt><dd class="num">{station.stats.slots_worked}</dd></div>
        <div><dt>Slots confirmed</dt><dd class="num ok-num">{station.stats.slots_confirmed}</dd></div>
      </dl>

      {#if station.by_band_mode}
        <h2 class="sub-head">Entities per band</h2>
        <div class="editor-scroll">
          <table class="slices">
            <thead>
              <tr>
                <th>Band</th>
                {#each station.by_band_mode.bands as b (b.key)}<th class="num">{b.key}</th>{/each}
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Worked</td>
                {#each station.by_band_mode.bands as b (b.key)}
                  <td class="num" class:zero={!b.worked}>{b.worked}</td>
                {/each}
              </tr>
              <tr>
                <td>Confirmed</td>
                {#each station.by_band_mode.bands as b (b.key)}
                  <td class="num ok-num" class:zero={!b.confirmed}>{b.confirmed}</td>
                {/each}
              </tr>
            </tbody>
          </table>
        </div>

        <h2 class="sub-head">Entities per mode</h2>
        <table class="slices">
          <thead>
            <tr>
              <th>Mode</th>
              {#each station.by_band_mode.modes as m (m.key)}<th class="num">{m.key}</th>{/each}
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>Worked</td>
              {#each station.by_band_mode.modes as m (m.key)}
                <td class="num" class:zero={!m.worked}>{m.worked}</td>
              {/each}
            </tr>
            <tr>
              <td>Confirmed</td>
              {#each station.by_band_mode.modes as m (m.key)}
                <td class="num ok-num" class:zero={!m.confirmed}>{m.confirmed}</td>
              {/each}
            </tr>
          </tbody>
        </table>
        <p class="hint note">
          Counts are <b>entities</b>, not QSOs: a band's figure is how many
          DXCC entities you have at least one contact with there. Digital
          modes share one DATA bucket, matching the DXCC award rules. A zero
          is left visible — an empty band is the most useful row here.
        </p>
      {/if}
    </div>
  {:else}
    <div class="card">
      <h2>Log statistics</h2>
      <p class="hint">
        No log loaded yet. Enter your ClubLog credentials above and press
        <b>Refresh log now</b> — the statistics appear here once it downloads.
      </p>
    </div>
  {/if}
</div>

<style>
  /* The band table is sixteen columns; it scrolls inside the card rather
     than widening the page, the same rule the System editors follow. */
  .editor-scroll {
    overflow-x: auto;
  }

  .sub-head {
    margin: 1.25rem 0 0.5rem;
    font-size: 0.95rem;
  }

  /* Svelte scopes component styles, so System.svelte's identical .stats
     block does not reach here — the six totals stacked one per line until
     this existed. Same shape on purpose: the two screens report the same
     numbers and should not look like different things. */
  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 0.9rem 2rem;
    margin: 0;
  }

  .stats dt {
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .stats dd {
    margin: 0.1rem 0 0;
    font-size: 1.05rem;
  }

  .stats dd.num {
    font-variant-numeric: tabular-nums;
  }

  .slices {
    width: auto;
    font-variant-numeric: tabular-nums;
  }

  .slices th,
  .slices td {
    padding: 0.2rem 0.75rem 0.2rem 0;
    white-space: nowrap;
  }

  .slices .num {
    text-align: right;
  }

  /* Confirmed is the number that counts for an award, so it reads as the
     positive one; worked beside it is the chase still open. */
  .ok-num {
    color: var(--accent);
  }

  /* A zero is deliberately quiet but present: an empty band is information,
     and blanking it would hide the gap worth working. */
  .zero {
    opacity: 0.35;
  }

  /* Sits between the card title and the fields it explains, so it takes the
     gap rather than adding one. */
  .intro {
    margin: -0.35rem 0 1rem;
    line-height: 1.5;
    max-width: 34rem;
  }

  .sub {
    margin: -0.35rem 0 0.7rem;
    line-height: 1.45;
    max-width: 34rem;
  }

  /* Column-major over four rows, so the server's FLAGGABLE order (the four
     New levels, then the four ? ones) lands as PAIRS: New DXCC beside ? DXCC,
     New Band beside ? Band. Reading across a row then compares the two rungs
     of the same axis, which is the comparison the operator actually makes.
     Row-major would have put New Band next to New DXCC — tidy, but it pairs
     nothing. */
  .levels {
    display: grid;
    grid-auto-flow: column;
    grid-template-rows: repeat(4, auto);
    grid-template-columns: repeat(2, minmax(8.5rem, 1fr));
    gap: 0.35rem 1rem;
    max-width: 24rem;
  }

  .levels label {
    gap: 0.45rem;
  }

  /* Too narrow to pair: one column, still in FLAGGABLE order. */
  @media (max-width: 30rem) {
    .levels {
      grid-auto-flow: row;
      grid-template-columns: 1fr;
      grid-template-rows: none;
    }
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
