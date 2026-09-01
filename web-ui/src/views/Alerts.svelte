<script lang="ts">
  // Alerts: the Telegram-side narrowing, and what actually went out.
  //
  // Same shape as Spots — collapsible rail on the left, table taking the rest —
  // because it IS the same shape of screen: a set of narrowings, and the rows
  // they produced. Sharing FilterRail rather than restyling one per screen
  // means the collapse, the badge and the breakpoint behave identically on
  // both, and the gesture is learned once.
  //
  // What the rail holds is not the same KIND of thing as Spots' though, and
  // the Save button is where that shows: these are account settings the server
  // stores, not a per-browser view. Until you press it, the rail is a draft.
  //
  // The bot token, chat id and cooldown moved to Settings › My station ›
  // Telegram in the 2026-08-29 cleanup — they are typed once. What stayed is
  // what you RETUNE while operating.
  //
  // Both halves live in one `notifications` row the server replaces wholesale,
  // so this page loads the WHOLE object on mount and writes it back with only
  // its own fields changed. Never send a partial: it would clear the
  // credentials the Settings page owns.
  //
  // This narrowing is INDEPENDENT of the Spots screen's: the point is to be
  // able to watch the whole band plan on screen while only being pinged for
  // one slice of it. The two controls look alike on purpose; the wording
  // ("ping me") is what says which one you are editing.
  import { api, hhmm } from '../lib/api';
  import { onMount } from 'svelte';
  import ChipGroup from '../lib/ChipGroup.svelte';
  import FilterRail from '../lib/FilterRail.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import {
    loadReference, bands, modes, levels, levelLabel,
  } from '../lib/reference.svelte';
  import { loadChase, chasedLevels } from '../lib/chase.svelte';

  // The ladder shows the classic eight plus only the awards this account
  // chases (Settings › My station › Awards) — an award nobody opted into
  // must not add rows here.
  let myLevels = $derived(chasedLevels(levels()));

  // Level key → the notify_* field that gates it. The server owns the ladder
  // and its order (AlertLevel::FLAGGABLE); this only maps key → field name.
  const FIELD: Record<string, string> = {
    newDXCC: 'notify_new_dxcc',
    newBand: 'notify_new_band',
    newMode: 'notify_new_mode',
    newSlot: 'notify_new_slot',
    unconfDXCC: 'notify_unconf_dxcc',
    unconfBand: 'notify_unconf_band',
    unconfMode: 'notify_unconf_mode',
    unconfSlot: 'notify_unconf_slot',
    newIOTA: 'notify_new_iota',
    newState: 'notify_new_state',
    newGrid: 'notify_new_grid',
    unconfIOTA: 'notify_unconf_iota',
    unconfState: 'notify_unconf_state',
    unconfGrid: 'notify_unconf_grid',
  };

  let cfg = $state<any>({
    telegram_enabled: false, telegram_bot_token: '', telegram_chat_id: '',
    cooldown_minutes: 15,
    notify_new_dxcc: true, notify_new_slot: true,
    notify_new_band: true, notify_new_mode: true,
    notify_unconf_dxcc: false, notify_unconf_slot: false,
    notify_unconf_band: false, notify_unconf_mode: false,
    // The award levels default ON here (chasing an award on Settings ›
    // Awards is the opt-in; this gate must not be a second one to find).
    notify_new_iota: true, notify_new_state: true, notify_new_grid: true,
    notify_unconf_iota: true, notify_unconf_state: true, notify_unconf_grid: true,
    notify_unconf_skip_worked: false, notify_unconf_lotw_only: false,
    notify_bands: [], notify_modes: [],
    notify_manual_only: false,
    notify_spotter_kind: 'all',
    notify_respect_band_mask: false,
  });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  // The two list fields ride as Sets so ChipGroup can bind them, and are
  // written back as arrays on save.
  let bandSel = $state<Set<string>>(new Set());
  let modeSel = $state<Set<string>>(new Set());

  // What has actually been sent to this account's Telegram.
  let sent = $state<any[]>([]);
  async function loadSent() {
    const r = await api('GET', '/api/me/alerts?limit=200');
    if (r.status === 200) sent = r.json.alerts ?? [];
  }

  onMount(async () => {
    await Promise.all([loadReference(), loadChase()]);
    const r = await api('GET', '/api/config/me/notifications');
    if (r.status === 200 && r.json) {
      cfg = { ...cfg, ...r.json };
      bandSel = new Set(r.json.notify_bands ?? []);
      modeSel = new Set(r.json.notify_modes ?? []);
    }
    await loadSent();
    // Alerts arrive while the page is open; a history that only updated on
    // reload would be the same invisibility this screen exists to fix.
    const t = setInterval(loadSent, 15000);
    return () => clearInterval(t);
  });

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/notifications', {
      ...cfg,
      cooldown_minutes: Number(cfg.cooldown_minutes) || 15,
      notify_bands: [...bandSel],
      notify_modes: [...modeSel],
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  let anyLevel = $derived(myLevels.some((l) => cfg[FIELD[l.key]]));

  // What the collapsed rail's badge reports — counted per CONTROL, the same
  // rule Spots uses, so the number means the same thing on both screens. The
  // eight-level ladder is ONE control, so anything short of all eight counts
  // once rather than eight times.
  let activeFilters = $derived(
    (myLevels.length && !myLevels.every((l) => cfg[FIELD[l.key]]) ? 1 : 0) +
      (cfg.notify_unconf_skip_worked || cfg.notify_unconf_lotw_only ? 1 : 0) +
      (modeSel.size ? 1 : 0) +
      (bandSel.size ? 1 : 0) +
      (cfg.notify_spotter_kind !== 'all' ? 1 : 0) +
      (cfg.notify_respect_band_mask ? 1 : 0),
  );
</script>

<div class="feedpage">
  <FilterRail activeCount={activeFilters}>
    <div class="railgroup">
      <span class="railhead">
        Ping me for
        <HelpTip label="Ping me for">
          <span class="para">
            This is the <b>Telegram</b> gate. It narrows, it never widens: a
            level only pings if <b>Settings › My station › Awards</b> allows
            your log to flag it in the first place — and an award not ticked
            there has no rows here at all.
          </span>
          <span class="para">
            Nothing here touches the Spots feed — untick a level and you will
            still see it on screen, you just won't be woken for it.
          </span>
        </HelpTip>
      </span>
      <div class="levels">
        {#each myLevels as l (l.key)}
          <label data-level={l.key}>
            <input type="checkbox" bind:checked={cfg[FIELD[l.key]]} />
            <span class="level-dot"></span>{l.label}
          </label>
        {/each}
      </div>
    </div>

    <div class="railgroup">
      <span class="railhead">
        For the ? levels
        <HelpTip label="For the ? levels">
          <span class="para">
            The <b>?</b> levels exist to hunt confirmations, and a
            confirmation needs the right station: some operators simply never
            QSL, and re-working one cannot turn an entity green.
          </span>
          <span class="para">
            Both ticks narrow only the <b>?</b> levels. With both on,
            only a call you have never worked that uploads to LoTW will ping
            — a station that can be worked <b>and</b> will confirm.
          </span>
          <span class="para">
            The <b>New</b> levels are untouched and always ping their own
            ticks: an ATNO is worth working whatever the QSL prospects.
          </span>
        </HelpTip>
      </span>
      <!-- docs/AWARDS.md phase 1. On the call, not the spot's provenance —
           and only on the ? half of the ladder: an ATNO is worth working
           whatever the QSL prospects. -->
      <label
        class="flabel"
        title="Hold ? pings for calls already in your log. A call you worked that never confirmed is a demonstrated non-QSLer — re-working them cannot confirm the entity."
      >
        <input type="checkbox" bind:checked={cfg.notify_unconf_skip_worked} />The call is
        new to my log
      </label>
      <label
        class="flabel"
        title="Hold ? pings for calls not on the LoTW users list — a LoTW user is the fast path to a confirmation."
      >
        <input type="checkbox" bind:checked={cfg.notify_unconf_lotw_only} />The call uses
        LoTW
      </label>
    </div>

    <ChipGroup stacked label="Modes" options={modes()} bind:selected={modeSel} />
    <ChipGroup stacked label="Bands" options={bands()} bind:selected={bandSel} />

    <div class="railgroup">
      <span class="railhead">
        Spotted by
        <HelpTip label="Spotted by">
          Who has to have made the spot for it to ping. The same control the
          <b>Spots</b> screen has, asked about Telegram — and independent of
          it, which is the point: watch every spot on screen, be woken for one
          slice of them.
        </HelpTip>
      </span>
      <div class="segmented" role="group" aria-label="Who made the spot">
        <button class:active={cfg.notify_spotter_kind === 'all'}
          onclick={() => (cfg.notify_spotter_kind = 'all')}
          title="Every spot, however it was heard.">All</button>
        <button class:active={cfg.notify_spotter_kind === 'human'}
          onclick={() => (cfg.notify_spotter_kind = 'human')}
          title="Only spots a person typed — skimmers (the -# callsigns) removed.">Human</button>
        <button class:active={cfg.notify_spotter_kind === 'skimmer'}
          onclick={() => (cfg.notify_spotter_kind = 'skimmer')}
          title="Only what the skimmers heard. A rare prefix usually shows up on a CW skimmer sweep before anyone types it.">Skimmer</button>
      </div>
    </div>

    <div class="railgroup">
      <span class="railhead">
        Only ping when
        <HelpTip label="Only ping when">
          <span class="para">
            Holds alerts for bands the sun says are not workable from your
            QTH right now. Needs a locator in <b>Settings › My station ›
            Locator &amp; grey line</b>.
          </span>
          <span class="para">
            <b>New DXCC always pings</b>, whatever the sun is doing — and so
            does a band the model says nothing about. That exemption is what
            makes this tick safe to enable: the worst it can do is hold a
            spot you could not have worked.
          </span>
        </HelpTip>
      </span>
      <!-- Milestone 4 of docs/PHASE-ROTATION-MASK.md. Narrowed separately from
           the Spots screen's own mask, like every other narrowing here: watch
           everything on screen, be woken only for what is workable. It fails
           open — no locator, or a band the model says nothing about, still
           pings — because a suppressed Telegram is a spot you never learn
           about at all. -->
      <label
        class="flabel"
        title="Hold alerts for bands the sun says are not workable from your QTH right now. Needs a locator in Settings › My station. New DXCC always pings whatever the sun is doing, and a band the model says nothing about always pings too."
      >
        <input type="checkbox" bind:checked={cfg.notify_respect_band_mask} />The band is
        plausibly open
      </label>
    </div>

    <div class="railgroup">
      <button class="primary wide" onclick={save} disabled={busy}>Save</button>
      {#if message}<p class="ok">{message}</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}
    </div>
  </FilterRail>

  <div class="feedmain">
    <!-- What actually went out. Before this the fan-out was invisible: a spot
         that was flagged, narrowed away, held by the cooldown, or refused by
         Telegram all looked the same from here — nothing arrived. -->
    <div class="stationline">
      <span class="who">Alerts sent</span>
      <HelpTip label="Alerts sent">
        The last {sent.length} Telegram alert{sent.length === 1 ? '' : 's'} for
        this account, newest first. Failed sends are kept and marked — a refused
        message is the row worth seeing.
      </HelpTip>
      {#if !cfg.telegram_enabled}
        <span class="warn">
          Telegram is off — turn it on in <b>Settings › My station › Telegram</b>.
        </span>
      {:else if !anyLevel}
        <span class="warn">No levels ticked — Telegram is on but nothing will ever ping.</span>
      {/if}
      <span class="counts"><span class="count muted">{sent.length} sent</span></span>
    </div>

    <div class="card feed">
      <div class="table-wrap">
        <!-- The feed's grid, so a sent alert reads as the spot it was: the same
             fixed widths, the same level tint, the same clip-with-a-hover.
             Shorter than Spots by the columns it has no use for. -->
        <table>
          <colgroup>
            <col class="c-time" /><col class="c-call" /><col class="c-spot" />
            <col class="c-src" /><col class="c-freq" /><col class="c-mode" />
            <col class="c-db" /><col class="c-band" /><col class="c-dxcc" />
            <col class="c-al" /><col class="c-status" />
          </colgroup>
          <thead>
            <tr>
              <th>Time</th>
              <th title="The station being spotted">DX</th>
              <th title="The station that heard it">DE</th>
              <th title="The feed that carried the spot">Source</th>
              <th title="Frequency in kHz">Freq</th>
              <th>Mode</th>
              <th title="Signal-to-noise, dB">dB</th>
              <th>Band</th><th>DXCC</th><th>Alert</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each sent as a}
              <tr data-level={a.level}>
                <td class="mono">{hhmm(a.time_unix)}Z</td>
                <td class="mono call">{a.callsign}</td>
                <td class="mono muted">{a.spotter || '—'}</td>
                <td class="muted" title={a.source}>{a.source}</td>
                <td class="mono">{(a.frequency_hz / 1000).toFixed(1)}</td>
                <td class="muted">{a.mode}</td>
                <!-- Alerts recorded before snr_db existed have no reading, and
                     say so: 0 dB is a real report, so a blank-looking zero
                     would be a plausible lie about a historical row. -->
                <td class="mono">{a.snr_db ?? '—'}</td>
                <td>{a.band}</td>
                <td title={a.dxcc_name}>{a.dxcc_name}</td>
                <td class="alert"
                  >{levelLabel(a.level)}{a.award_ref ? ` ${a.award_ref}` : ''}</td
                >
                <!-- Shown either way, not just on failure: a column that is
                     blank on a good row cannot be told from a column that is
                     broken, and "did it actually go out" is the question this
                     whole table exists to answer. -->
                <td class="status">
                  {#if a.delivered}
                    <span class="ok-tick" title="Delivered to Telegram">✓</span>
                  {:else}
                    <span class="err failed" title="Telegram refused this one: {a.error || 'no reason given'}"
                      >Failed</span
                    >
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if sent.length === 0}
          <p class="empty hint">
            Nothing sent yet. Alerts appear here once Telegram is switched on
            and a spot matches a level you have ticked.
          </p>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  /* Rail | table, exactly as Spots. See Dashboard.svelte for why the second
     track needs `minmax(0, 1fr)` rather than `1fr`. */
  .feedpage {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
  }

  .feedmain {
    min-width: 0;
    padding: 0.9rem 1.25rem 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .railgroup {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  /* Sized exactly as the Spots rail's copy — three segments across a 12rem
     rail. Two screens, one control, one look. */
  .segmented {
    width: 100%;
  }

  .segmented button {
    flex: 1;
    padding: 0.15rem 0.2rem;
    font-size: 0.75rem;
  }

  .railhead {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    font-size: 0.62rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.11em;
    color: var(--muted);
  }

  /* One column in a 12rem rail. The two-column pairing the Settings ladder
     uses has no room here, and the server's FLAGGABLE order already reads as
     the four New levels followed by their four ? counterparts. */
  .levels {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .levels label {
    gap: 0.4rem;
    font-size: 0.8rem;
  }

  .flabel {
    color: var(--muted);
    font-size: 0.78rem;
    gap: 0.3rem;
    align-items: flex-start;
    line-height: 1.35;
  }

  .wide {
    width: 100%;
  }

  .warn {
    color: var(--warn);
    font-size: var(--fs-hint);
  }

  .stationline {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0.25rem 1.1rem;
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .stationline .who {
    font-size: 1.05rem;
    font-weight: 600;
    color: CanvasText;
  }

  .counts {
    margin-left: auto;
  }

  .count {
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }

  .feed {
    padding: 0;
    min-height: 14rem;
    overflow: hidden;
  }

  .table-wrap {
    overflow: auto;
    max-height: calc(100vh - 8.5rem);
  }

  table {
    table-layout: fixed;
    min-width: 46rem;
  }

  /* Near the Spots widths, minus the 0.75rem each header there reserves for a
     sort caret — this table does not sort, so it was 6rem of pure air.
     
     ONE ELASTIC COLUMN, which is what this table was missing: with all nine
     fixed it summed past the card and truncated on the right at any window
     under about 1200px, since nothing could give. Now the fixed columns come
     to 42rem and Source · Spotter takes whatever is left — generous on a wide
     window, clipped with a hover on a narrow one. `min-width` on the table is
     the floor below which it scrolls instead of crushing. */
  col.c-time { width: 4.75rem; }
  col.c-call { width: 6.75rem; }
  col.c-spot { width: 6rem; }
  col.c-src  { width: 8rem; }
  col.c-freq { width: 5.5rem; }
  col.c-mode { width: 4.25rem; }
  col.c-db   { width: 3rem; }
  col.c-band { width: 3.5rem; }
  col.c-dxcc { width: 11.5rem; }
  col.c-al   { width: 5.75rem; }
  /* Last and elastic — it takes the slack so the table always fills its card
     rather than truncating, and "Failed" is short enough that the extra room
     reads as margin. */
  col.c-status { width: auto; }

  td {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* --- Alignment ---
     Fixed columns leave slack, and left-aligning every value pinned each one
     to the far edge of its box: "SYRIA" sat at the left of an 11.5rem DXCC
     column with an inch of nothing before the next field, so the row read as
     scattered rather than as a row. Centred, each value sits in its own cell.

     The two NUMERIC columns are the exception and stay right-aligned: kHz and
     dB are read by comparing them down the column, and centring
     "7040.0" over "14090.7" puts the decimal points in different places,
     which is the one thing tabular figures exist to prevent.

     Free prose stays left — a centred paragraph has no edge to read from. */
  th,
  td {
    text-align: center;
  }

  /* Freq and dB — columns 5 and 7, the same positions they hold on Spots. */
  th:nth-child(5), td:nth-child(5),
  th:nth-child(7), td:nth-child(7) {
    text-align: right;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    /* Opaque: rows scroll underneath it. */
    background: var(--card-bg);
    user-select: none;
    padding-top: 0.6rem;
    padding-bottom: 0.4rem;
    overflow: hidden;
  }

  th:first-child,
  td:first-child {
    padding-left: 1rem;
  }

  th:last-child,
  td:last-child {
    padding-right: 1rem;
  }

  .call {
    font-weight: 600;
  }

  .alert {
    font-weight: 600;
  }

  /* The tint. `data-level` on the row resolves `--lvl`/`--lvl-bg` from
     app.css's level table, but painting with them is per-component — without
     these two rules the rows sit untinted, vocabulary claimed but not
     delivered. Every sent alert was flagged by definition, so no gate class. */
  tr[data-level] td {
    background: var(--lvl-bg);
  }

  tr[data-level] .alert {
    color: var(--lvl);
  }

  .failed {
    font-size: 0.72rem;
    font-weight: 600;
    cursor: help;
  }

  /* Quiet on purpose — it is the expected answer, and a column of bright ticks
     would pull the eye away from the failures, which are the rows worth
     seeing. */
  .ok-tick {
    color: var(--ok);
    opacity: 0.55;
    cursor: help;
  }

  .status {
    white-space: nowrap;
  }

  .empty {
    margin: 0;
    padding: 1.5rem 1rem;
    text-align: center;
  }

  p {
    margin: 0.4rem 0 0;
  }
</style>
