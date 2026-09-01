<script lang="ts">
  // Settings › My station › ClubLog account — the credentials, and what the
  // log they fetch is allowed to flag.
  //
  // The two are one page because they are one stored object and one Save, and
  // because separating them was actively misleading: on its own rail page
  // called "Alert levels", the ladder sat next to an identical-looking list on
  // the Alerts tab with no way to tell which was which. They are NOT the same
  // control:
  //
  //   * THIS one is the classifier gate. Switch a level off here and it is
  //     never assigned at all — it vanishes from the spots feed AND from
  //     Telegram, because nothing downstream ever sees it. It rides on the
  //     ClubLog config because your log is what decides the level.
  //   * The Alerts tab's "Ping me for" only narrows what this already allows,
  //     and only for Telegram. The feed keeps showing everything.
  //
  // Under "ClubLog account", the ladder reads as what it is: what your log
  // flags. That is the whole reason it moved back here.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import { loadReference, levels } from '../../lib/reference.svelte';

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

  // Level key → the classifier toggle that decides whether a spot is ever
  // flagged as that level at all. The server owns the ladder and its order
  // (AlertLevel::FLAGGABLE); this only maps key → field name.
  const FIELD: Record<string, string> = {
    newDXCC: 'alert_new_dxcc',
    newBand: 'alert_new_band',
    newMode: 'alert_new_mode',
    newSlot: 'alert_new_slot',
    unconfDXCC: 'alert_unconf_dxcc',
    unconfBand: 'alert_unconf_band',
    unconfMode: 'alert_unconf_mode',
    unconfSlot: 'alert_unconf_slot',
    newIOTA: 'alert_new_iota',
    newState: 'alert_new_state',
    newGrid: 'alert_new_grid',
    unconfIOTA: 'alert_unconf_iota',
    unconfState: 'alert_unconf_state',
    unconfGrid: 'alert_unconf_grid',
  };

  let cfg = $state<any>({
    callsign: '', email: '', app_password: '', refresh_hours: 24,
    lotw_login: '', lotw_password: '',
    alert_new_dxcc: true, alert_new_band: true, alert_new_mode: true, alert_new_slot: true,
    alert_unconf_dxcc: false, alert_unconf_band: false,
    alert_unconf_mode: false, alert_unconf_slot: false,
    // The award axes (docs/AWARDS.md): a ticked pair IS the award selector.
    alert_new_iota: false, alert_new_state: false, alert_new_grid: false,
    alert_unconf_iota: false, alert_unconf_state: false, alert_unconf_grid: false,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    await loadReference();
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
  });

  async function save() {
    busy = true; message = ''; error = '';
    // The <select> yields strings; the server's refresh_hours is an integer and
    // a quoted "24" fails its deserialize. Coerced on save, not on bind, so the
    // select still matches the loaded value by identity.
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
      message = `Refreshed: ${r.json.qso_count} QSOs, ${r.json.dxcc_count} DXCC entities. See Stats › ClubLog.`;
    } else {
      message = '';
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  let anyLevel = $derived(levels().some((l) => cfg[FIELD[l.key]]));
</script>

<div class="card">
  <h2>
    ClubLog account
    <HelpTip label="ClubLog account">
      Your log drives the New / ? highlighting — only for your account. The
      <b>ClubLog API key</b> is not here: it only fetches the shared DXCC prefix
      database, so it is one server-wide setting under <b>Server › Reference
      data</b>. These credentials download <em>your</em> log.
    </HelpTip>
  </h2>
  <div class="settings-form">
    <span class="label">Callsign</span>
    <input bind:value={cfg.callsign} autocapitalize="characters" />
    <span class="label">Email</span>
    <input bind:value={cfg.email} />
    <span class="label">App password</span>
    <input type="password" bind:value={cfg.app_password} />
    <span class="label">
      Auto-refresh
      <HelpTip label="Auto-refresh">
        {#if cfg.refresh_hours > 0}
          Re-downloads your log in the background, so a QSO worked today stops
          showing as New DXCC tomorrow. The button below still works any time.
        {:else}
          Your log will only change when you press <b>Refresh log now</b> —
          anything you work keeps alerting as new until you do.
        {/if}
      </HelpTip>
    </span>
    <select bind:value={cfg.refresh_hours}>
      {#each INTERVALS as [hours, label] (hours)}
        <option value={hours}>{label}</option>
      {/each}
    </select>
    <span class="label">
      LoTW username
      <HelpTip label="LoTW username">
        <span class="para">
          Optional, for the <b>IOTA / State / Grid</b> awards below: your
          LoTW QSL report is the one source that says which state, grid and
          island your confirmations came from — ClubLog's export cannot
          carry those fields.
        </span>
        <span class="para">
          Your LoTW <b>website</b> login, sent only to lotw.arrl.org, stored
          like the ClubLog credentials above (README §Secrets). Leave both
          blank and the three awards track worked-only.
        </span>
      </HelpTip>
    </span>
    <input bind:value={cfg.lotw_login} autocapitalize="characters" />
    <span class="label">LoTW password</span>
    <input type="password" bind:value={cfg.lotw_password} />
  </div>

  <h2>
    Levels my log flags
    <HelpTip label="Levels my log flags">
      <span class="para">
        Which levels this account flags <b>at all</b>. <b>New</b> means never
        worked; <b>?</b> means worked and still not confirmed — the QSL gap you
        close by working it again.
      </span>
      <span class="para">
        The widest of the three controls, and the reason it belongs with your
        log: a level switched off here is never assigned, so it disappears from
        the spots feed <em>and</em> from Telegram. The <b>Alerts</b> tab's "ping
        me for" only narrows what this already allows, and only for Telegram —
        the feed still shows everything.
      </span>
    </HelpTip>
  </h2>
  <!-- Column-major over seven rows, so the server's FLAGGABLE order (the
       seven New levels, then the seven ? ones) lands as PAIRS: New DXCC
       beside ? DXCC, New Grid beside ? Grid. Reading across a row then
       compares the two rungs of the same axis, which is the comparison the
       operator actually makes. Row-major would have put New Band next to
       New DXCC — tidy, but it pairs nothing.

       The IOTA / State / Grid pairs are also the award selector: tick a
       pair and that award classifies, spot data allowing (grids ride the
       feed already; State needs the FCC table under Server › Reference
       data; IOTA reads cluster comments). -->
  <div class="levels">
    {#each levels() as l (l.key)}
      <label data-level={l.key}>
        <input type="checkbox" bind:checked={cfg[FIELD[l.key]]} />
        <span class="level-dot"></span>{l.label}
      </label>
    {/each}
  </div>
  {#if !anyLevel}
    <p class="warn">No levels ticked — nothing will ever be flagged, on screen or in Telegram.</p>
  {/if}

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
    <button onclick={refresh} disabled={busy}>Refresh log now</button>
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  .levels {
    display: grid;
    grid-auto-flow: column;
    grid-template-rows: repeat(7, auto);
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

  .warn {
    color: var(--warn);
    font-size: var(--fs-hint);
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
