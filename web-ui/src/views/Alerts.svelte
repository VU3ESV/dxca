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

  // The notify_* field that gates a level comes FROM THE SERVER, on the
  // level itself (`NotifyUserConfig::notify_field`). This used to be a
  // table typed out here, and it was never extended when WAZ and the
  // Marathon joined the ladder in 2.19.0: their three rows looked up
  // nothing, so all three bound to the same `cfg[undefined]` — ticking DX
  // Marathon appeared to tick both Zone rows — and no save carried their
  // fields, so the server's default-on put them straight back. The only
  // way to stop those pings was to drop the award on Settings › Awards.
  //
  // A row with no field is dropped rather than drawn: a control that
  // cannot say no is worse than one that is not there.
  const fieldOf = (l: { notifyField?: string | null }) => l.notifyField ?? '';

  /* Short names for the delivery channels. The chip has to fit beside three
     others, so "Telegram" is TG — the tooltip carries the full story. */
  const CHANNEL_LABEL: Record<string, string> = {
    telegram: 'TG',
    flex: 'Flex',
    tci: 'TCI',
  };
  const channelLabel = (n: string) => CHANNEL_LABEL[n] ?? n.toUpperCase();
  const channelTitle = (ch: { name: string; target?: string; ok: boolean; error?: string }) =>
    `${channelLabel(ch.name)}${ch.target ? ` ${ch.target}` : ''} — ` +
    (ch.ok ? 'accepted' : ch.error || 'refused');

  // The ladder shows the classic eight plus only the awards this account
  // chases (Settings › My station › Awards) — an award nobody opted into
  // must not add rows here.
  let myLevels = $derived(chasedLevels(levels()).filter((l) => fieldOf(l)));

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
    notify_new_zone: true, notify_unconf_zone: true, notify_marathon: true,
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

  // What has actually been sent, to every channel this account uses.
  let sent = $state<any[]>([]);

  /* How many rows to FETCH. Separate from the filters below, which narrow
     what was fetched — a filter that could only see 20 rows would quietly
     lie about how many matches exist.

     "All" asks for more than the server will ever hold and lets it clamp
     (`q.limit.min(500)`), rather than hard-coding 500 here. Today that makes
     All and 500 the same set — the server keeps `ALERT_HISTORY_MAX = 500`
     rows per account and prunes on every insert — but if that cap is ever
     raised, All follows it without this file being touched. The two options
     must not share a value: `bind:value` matches on it, so a duplicate would
     leave "All" selectable but never shown as selected. */
  const LIMITS = [
    { n: 20, label: '20' },
    { n: 50, label: '50' },
    { n: 100, label: '100' },
    { n: 500, label: '500' },
    { n: 100000, label: 'All' },
  ];
  let limit = $state(100);

  async function loadSent() {
    const r = await api('GET', `/api/me/alerts?limit=${limit}`);
    if (r.status === 200) sent = r.json.alerts ?? [];
  }

  /* Per-column narrowing of what is on screen. Client-side on purpose: the
     whole history is at most 500 rows, so filtering here is instant and
     costs no round trip — and unlike the rail on the left, NONE of this is
     saved. The rail edits account settings; this is a lens on the table. */
  let fDx = $state('');
  let fDe = $state('');
  let fSource = $state('');
  let fMode = $state('');
  let fBand = $state('');
  let fDxcc = $state('');
  let fLevel = $state('');
  let fChannel = $state('');
  let fStatus = $state('');

  /* Options come from the rows actually loaded, not from the reference
     tables: a band this account has never been alerted on is a dead option,
     and a dropdown of dead options is worse than a short one. */
  const uniq = (rows: any[], pick: (r: any) => string) =>
    [...new Set(rows.map(pick).filter(Boolean))].sort();
  let srcOpts = $derived(uniq(sent, (r) => r.source));
  let modeOpts = $derived(uniq(sent, (r) => r.mode));
  let bandOpts = $derived(uniq(sent, (r) => r.band));
  let levelOpts = $derived(uniq(sent, (r) => r.level));
  let chanOpts = $derived(
    [...new Set(sent.flatMap((r: any) => (r.channels ?? []).map((c: any) => c.name)))].sort(),
  );

  const like = (v: unknown, f: string) =>
    !f || String(v ?? '').toLowerCase().includes(f.trim().toLowerCase());

  let shown = $derived(
    sent.filter((a: any) => {
      if (!like(a.callsign, fDx)) return false;
      if (!like(a.spotter, fDe)) return false;
      if (fSource && a.source !== fSource) return false;
      if (fMode && a.mode !== fMode) return false;
      if (fBand && a.band !== fBand) return false;
      if (!like(a.dxcc_name, fDxcc)) return false;
      if (fLevel && a.level !== fLevel) return false;
      // "Sent to" matches the channel being PRESENT, whether or not it
      // accepted — "show me everything that went at this radio" is the
      // question, and Status answers the other one.
      if (fChannel && !(a.channels ?? []).some((c: any) => c.name === fChannel)) return false;
      if (fStatus === 'ok' && !a.delivered) return false;
      if (fStatus === 'failed' && a.delivered) return false;
      return true;
    }),
  );

  let filtering = $derived(
    !!(fDx || fDe || fSource || fMode || fBand || fDxcc || fLevel || fChannel || fStatus),
  );

  function clearFilters() {
    fDx = fDe = fSource = fMode = fBand = fDxcc = fLevel = fChannel = fStatus = '';
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

  let anyLevel = $derived(myLevels.some((l) => cfg[fieldOf(l)]));

  // What the collapsed rail's badge reports — counted per CONTROL, the same
  // rule Spots uses, so the number means the same thing on both screens. The
  // eight-level ladder is ONE control, so anything short of all eight counts
  // once rather than eight times.
  let activeFilters = $derived(
    (myLevels.length && !myLevels.every((l) => cfg[fieldOf(l)]) ? 1 : 0) +
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
            <input type="checkbox" bind:checked={cfg[fieldOf(l)]} />
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
        The last {sent.length} alert{sent.length === 1 ? '' : 's'} for this
        account, newest first, across every channel — Telegram and each
        configured radio. Failures are kept and marked: a refused alert is the
        row worth seeing. The boxes under the headings narrow what is on
        screen and are not saved; <b>Show</b> changes how many rows are
        fetched. The server keeps at most 500 per account, so 500 and All are
        the same set.
      </HelpTip>
      {#if !cfg.telegram_enabled}
        <span class="warn">
          Telegram is off — turn it on in <b>Settings › My station › Telegram</b>.
        </span>
      {:else if !anyLevel}
        <span class="warn">No levels ticked — Telegram is on but nothing will ever ping.</span>
      {/if}
      <span class="counts">
        <label class="showsel" title="How many rows to fetch. The server keeps at most 500 per account.">
          Show
          <select bind:value={limit} onchange={loadSent}>
            {#each LIMITS as l}
              <option value={l.n}>{l.label}</option>
            {/each}
          </select>
        </label>
        {#if filtering}
          <button class="clearf" onclick={clearFilters} title="Clear every column filter"
            >Clear filters</button
          >
          <span class="count muted">{shown.length} of {sent.length}</span>
        {:else}
          <span class="count muted">{sent.length} sent</span>
        {/if}
      </span>
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
            <col class="c-al" /><col class="c-chan" /><col class="c-status" />
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
              <th title="The channels this alert was offered to">Sent to</th>
              <th>Status</th>
            </tr>
            <!-- Narrowing sits under the heading it narrows, so the column and
                 its control cannot be mismatched. Free text where the values
                 are open (callsigns, entity names), a list where they are not
                 — a dropdown you can mistype is a filter that silently
                 matches nothing. Time, Freq and dB have no control: a
                 substring of a frequency is not a question anyone asks. -->
            <tr class="filters">
              <td></td>
              <td><input type="text" bind:value={fDx} placeholder="call" aria-label="Filter by DX callsign" /></td>
              <td><input type="text" bind:value={fDe} placeholder="de" aria-label="Filter by spotter" /></td>
              <td>
                <select bind:value={fSource} aria-label="Filter by source">
                  <option value="">any</option>
                  {#each srcOpts as o}<option value={o}>{o}</option>{/each}
                </select>
              </td>
              <td></td>
              <td>
                <select bind:value={fMode} aria-label="Filter by mode">
                  <option value="">any</option>
                  {#each modeOpts as o}<option value={o}>{o}</option>{/each}
                </select>
              </td>
              <td></td>
              <td>
                <select bind:value={fBand} aria-label="Filter by band">
                  <option value="">any</option>
                  {#each bandOpts as o}<option value={o}>{o}</option>{/each}
                </select>
              </td>
              <td><input type="text" bind:value={fDxcc} placeholder="entity" aria-label="Filter by DXCC entity" /></td>
              <td>
                <select bind:value={fLevel} aria-label="Filter by alert level">
                  <option value="">any</option>
                  {#each levelOpts as o}<option value={o}>{levelLabel(o)}</option>{/each}
                </select>
              </td>
              <td>
                <select bind:value={fChannel} aria-label="Filter by channel">
                  <option value="">any</option>
                  {#each chanOpts as o}<option value={o}>{channelLabel(o)}</option>{/each}
                </select>
              </td>
              <td>
                <select bind:value={fStatus} aria-label="Filter by delivery status">
                  <option value="">any</option>
                  <option value="ok">Delivered</option>
                  <option value="failed">Failed</option>
                </select>
              </td>
            </tr>
          </thead>
          <tbody>
            {#each shown as a}
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
                <!-- One chip per channel, because they fail independently:
                     a radio that is switched off must not make a delivered
                     Telegram look broken, and with four radios configurable
                     "it failed" is not a useful answer without the address. -->
                <td class="chans">
                  {#if a.channels?.length}
                    {#each a.channels as ch}
                      <span class="chan" class:bad={!ch.ok} title={channelTitle(ch)}
                        >{channelLabel(ch.name)}</span
                      >
                    {/each}
                  {:else}
                    <span
                      class="muted"
                      title="Recorded before DXCA logged which channel an alert went to">—</span
                    >
                  {/if}
                </td>
                <!-- Shown either way, not just on failure: a column that is
                     blank on a good row cannot be told from a column that is
                     broken, and "did it actually go out" is the question this
                     whole table exists to answer. -->
                <td class="status">
                  {#if a.delivered}
                    <span class="ok-tick" title="Accepted by every channel it was sent to">✓</span>
                  {:else}
                    <span class="err failed" title={a.error || 'no reason given'}>Failed</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <!-- Two different nothings, and conflating them would send you to
             the wrong page: an empty history means no alert has fired, an
             empty filter means one has and you cannot see it. -->
        {#if sent.length === 0}
          <p class="empty hint">
            Nothing sent yet. Alerts appear here once a spot matches a level
            you have ticked and at least one channel — Telegram or a radio —
            is switched on.
          </p>
        {:else if shown.length === 0}
          <p class="empty hint">
            No alert matches these filters. <button class="linkish" onclick={clearFilters}
              >Clear them</button
            > to see all {sent.length}.
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
  /* Four radios plus Telegram is the realistic worst case; past that the
     cell scrolls rather than pushing Status off the card. */
  col.c-chan { width: 9rem; }
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

  /* The filter row. Deliberately quieter than the headings above it and the
     data below: it is scaffolding, and a row of bright controls would compete
     with the rows it exists to reveal. app.css already styles `input, select`
     with the real tokens, so this only resizes them to the column — restating
     the colours here is how a control drifts out of the theme. */
  .filters td {
    padding: 0.15rem 0.3rem;
    border-bottom: 1px solid var(--border);
    overflow: visible;
  }

  .filters input,
  .filters select {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    font-size: 0.72rem;
    padding: 0.1rem 0.25rem;
    border-radius: 4px;
  }

  .filters input::placeholder {
    color: var(--muted);
    opacity: 0.7;
  }

  .showsel {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: var(--fs-hint);
    color: var(--muted);
  }

  .showsel select {
    font-size: 0.78rem;
    padding: 0.1rem 0.3rem;
  }

  /* Only drawn while something is filtered, so it never sits there as a
     control with nothing to undo. */
  .clearf,
  .linkish {
    font: inherit;
    font-size: var(--fs-hint);
    color: var(--accent);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }

  .chans {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    align-items: center;
    overflow: hidden;
  }

  /* Reads as a label, not a button: this is a record of what happened, and
     nothing here is clickable. */
  .chan {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    padding: 0.05rem 0.3rem;
    border-radius: 0.2rem;
    border: 1px solid var(--ok);
    color: var(--ok);
    opacity: 0.75;
    cursor: help;
    white-space: nowrap;
  }

  /* Full strength, unlike the quiet success chip — a channel that refused is
     the thing worth spotting in a column of them. */
  .chan.bad {
    border-color: var(--err);
    color: var(--err);
    opacity: 1;
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
