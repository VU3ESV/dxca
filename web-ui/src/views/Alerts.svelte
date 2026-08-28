<script lang="ts">
  // My alerts (plan §8 page 5): Telegram setup + test, cooldown, and the
  // Telegram-side narrowing — which of the eight levels ping, and on which
  // bands and mode classes.
  //
  // This narrowing is INDEPENDENT of the Spots screen's: the point is to be
  // able to watch the whole band plan on screen while only being pinged for
  // one slice of it. The two controls look alike on purpose; the wording
  // ("ping me") is what says which one you are editing.
  import { api, hhmm } from '../lib/api';
  import { onMount } from 'svelte';
  import ChipGroup from '../lib/ChipGroup.svelte';
  import { loadReference, bands, modes, levels, levelLabel } from '../lib/reference.svelte';

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
  };

  let cfg = $state<any>({
    telegram_enabled: false, telegram_bot_token: '', telegram_chat_id: '',
    cooldown_minutes: 15,
    notify_new_dxcc: true, notify_new_slot: true,
    notify_new_band: true, notify_new_mode: true,
    notify_unconf_dxcc: false, notify_unconf_slot: false,
    notify_unconf_band: false, notify_unconf_mode: false,
    notify_bands: [], notify_modes: [],
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
    await loadReference();
    const r = await api('GET', '/api/config/me/notifications');
    if (r.status === 200 && r.json) {
      cfg = { ...cfg, ...r.json };
      bandSel = new Set(r.json.notify_bands ?? []);
      modeSel = new Set(r.json.notify_modes ?? []);
    }
    await loadSent();
    // Alerts arrive while the page is open; a history that only updated on
    // reload would be the same invisibility this card exists to fix.
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

  async function test() {
    busy = true; message = ''; error = '';
    const r = await api('POST', '/api/telegram/test');
    busy = false;
    if (r.status === 200) message = 'Test message sent — check Telegram.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  let anyLevel = $derived(levels().some((l) => cfg[FIELD[l.key]]));
</script>

<div class="page narrow">
  <div class="card">
    <h2>My alerts — Telegram</h2>
    <label class="enable">
      <input type="checkbox" bind:checked={cfg.telegram_enabled} />Enable Telegram alerts
    </label>
    <div class="settings-form">
      <span class="label">Bot token</span>
      <input type="password" bind:value={cfg.telegram_bot_token} placeholder="from @BotFather" />
      <span class="label">Chat ID</span>
      <input bind:value={cfg.telegram_chat_id} />
      <span class="label">Cooldown (min)</span>
      <input class="short" type="number" min="5" max="60" bind:value={cfg.cooldown_minutes} />
    </div>

    <h2>Ping me for</h2>
    <p class="hint sub">
      A level only pings if <b>My ClubLog</b> is allowed to flag it in the
      first place — this narrows, it never widens.
    </p>
    <div class="levels">
      {#each levels() as l (l.key)}
        <label data-level={l.key}>
          <input type="checkbox" bind:checked={cfg[FIELD[l.key]]} />
          <span class="level-dot"></span>{l.label}
        </label>
      {/each}
    </div>
    {#if cfg.telegram_enabled && !anyLevel}
      <p class="warn">No levels ticked — Telegram is on but nothing will ever ping.</p>
    {/if}

    <h2>Only on</h2>
    <div class="pickers">
      <ChipGroup label="Modes" options={modes()} bind:selected={modeSel} />
      <ChipGroup label="Bands" options={bands()} bind:selected={bandSel} />
    </div>

    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
      <button onclick={test} disabled={busy}>Send test message</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>

  <!-- What actually went out. Before this the fan-out was invisible: a spot
       that was flagged, narrowed away, held by the cooldown, or refused by
       Telegram all looked the same from here — nothing arrived. -->
  <div class="card">
    <h2>Alerts sent</h2>
    <p class="hint sub">
      The last {sent.length} Telegram alert{sent.length === 1 ? '' : 's'} for this
      account, newest first. Failed sends are kept and marked — a refused
      message is the row worth seeing.
    </p>
    {#if sent.length === 0}
      <p class="hint">
        Nothing sent yet. Alerts appear here once Telegram is switched on and
        a spot matches a level you have ticked above.
      </p>
    {:else}
      <div class="table-scroll">
        <table class="sent">
          <thead>
            <tr>
              <th>Time</th><th>DX Call</th><th>kHz</th><th>Mode</th>
              <th>Band</th><th>DXCC</th><th>Level</th><th>Source</th><th></th>
            </tr>
          </thead>
          <tbody>
            {#each sent as a}
              <tr data-level={a.level}>
                <td class="mono">{hhmm(a.time_unix)}Z</td>
                <td class="mono call">{a.callsign}</td>
                <td class="mono">{(a.frequency_hz / 1000).toFixed(1)}</td>
                <td class="muted">{a.mode}</td>
                <td>{a.band}</td>
                <td>{a.dxcc_name}</td>
                <td class="alert">{levelLabel(a.level)}</td>
                <td class="muted">{a.source}</td>
                <td>
                  {#if !a.delivered}
                    <span class="err failed" title={a.error}>failed</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>

<style>
  /* Same row vocabulary as the Spots feed — the level tint via [data-level]
     and a nowrap dense table — so a sent alert reads as the spot it was. */
  .table-scroll {
    overflow-x: auto;
  }

  .sent {
    width: 100%;
    font-size: 0.85rem;
  }

  .sent .call {
    font-weight: 600;
  }

  .sent .alert {
    font-weight: 600;
  }

  /* The tint itself. `data-level` on the row resolves `--lvl`/`--lvl-bg`
     from app.css's level table, but painting with them is per-component
     (Dashboard's rules are scoped to Dashboard) — without these two rules
     the rows sat untinted, vocabulary claimed but not delivered. Every
     sent alert was flagged by definition, so no gate class. */
  .sent tr[data-level] td {
    background: var(--lvl-bg);
  }

  .sent tr[data-level] .alert {
    color: var(--lvl);
  }

  .failed {
    font-size: 0.75rem;
    cursor: help;
  }

  .enable {
    margin-bottom: 0.9rem;
  }

  .short {
    width: 6rem;
  }

  .sub {
    margin: -0.35rem 0 0.7rem;
    line-height: 1.45;
  }

  /* Column-major over four rows, so FLAGGABLE order lands as pairs — New
     DXCC beside ? DXCC, and so on. Same shape as My ClubLog's, because the
     two lists ask the same question about the same eight levels. */
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

  @media (max-width: 30rem) {
    .levels {
      grid-auto-flow: row;
      grid-template-columns: 1fr;
      grid-template-rows: none;
    }
  }

  .pickers {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .warn {
    color: var(--warn);
    font-size: var(--fs-hint);
    margin: 0.5rem 0 0;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
