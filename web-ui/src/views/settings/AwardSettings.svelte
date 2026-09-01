<script lang="ts">
  // Settings › My station › Awards — which awards this account chases, and
  // which levels each one may flag.
  //
  // One page for the whole question, on Manoj's direction (2026-09-01): the
  // award choice is an AWARDS setting, not a footnote on the credentials
  // page, and the rest of the app must not grow controls for awards nobody
  // opted into. Chasing an award here is what makes its levels exist
  // anywhere else — the Spots chips, the Alerts ladder, the Stats card all
  // filter through `lib/chase`.
  //
  // These are the CLASSIFIER gates (the widest control): a level off here
  // is never assigned at all, on screen or in Telegram. The Alerts tab's
  // "Ping me for" narrows what this allows, for Telegram only — the same
  // two-control story the ladder had when it lived on the ClubLog page.
  //
  // "Chasing" is not a stored flag of its own: an award is chased exactly
  // when either of its two levels is on, so there is no way for a selector
  // and the levels to disagree.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import { status, refreshStatus } from '../../lib/status.svelte';
  import { setChase } from '../../lib/chase.svelte';

  // The classic DXCC ladder, column-major pairs (New beside its ?).
  const DXCC_FIELD: Record<string, string> = {
    newDXCC: 'alert_new_dxcc',
    newBand: 'alert_new_band',
    newMode: 'alert_new_mode',
    newSlot: 'alert_new_slot',
    unconfDXCC: 'alert_unconf_dxcc',
    unconfBand: 'alert_unconf_band',
    unconfMode: 'alert_unconf_mode',
    unconfSlot: 'alert_unconf_slot',
  };
  const DXCC_ORDER = [
    'newDXCC', 'newBand', 'newMode', 'newSlot',
    'unconfDXCC', 'unconfBand', 'unconfMode', 'unconfSlot',
  ];
  const DXCC_LABEL: Record<string, string> = {
    newDXCC: 'NEW DXCC', newBand: 'New Band', newMode: 'New Mode', newSlot: 'New Slot',
    unconfDXCC: '? DXCC', unconfBand: '? Band', unconfMode: '? Mode', unconfSlot: '? Slot',
  };

  // The chaseable awards: their level pair, and what each runs on.
  const AWARDS = [
    {
      id: 'iota', name: 'IOTA', newField: 'alert_new_iota', unconfField: 'alert_unconf_iota',
      newKey: 'newIOTA', unconfKey: 'unconfIOTA', newLabel: 'New IOTA', unconfLabel: '? IOTA',
      what: 'Island groups, from the reference a cluster spot’s comment announces (AS-153), '
          + 'validated against the IOTA directory. FT8 decodes never name an island, so these '
          + 'levels ride cluster spots only.',
    },
    {
      id: 'was', name: 'WAS', newField: 'alert_new_state', unconfField: 'alert_unconf_state',
      newKey: 'newState', unconfKey: 'unconfState', newLabel: 'New State', unconfLabel: '? State',
      what: 'The fifty US states, looked up in the FCC licence database (DC counts as Maryland). '
          + 'The FCC knows the licence address, so a W6 living in Ohio reads as California.',
    },
    {
      id: 'vucc', name: 'VUCC', newField: 'alert_new_grid', unconfField: 'alert_unconf_grid',
      newKey: 'newGrid', unconfKey: 'unconfGrid', newLabel: 'New Grid', unconfLabel: '? Grid',
      what: '4-character grid squares, per band, 50 MHz and up only — the ARRL rule. The grid '
          + 'comes from the cluster comment or an FT8 CQ; RR73 is always a sign-off, never a square.',
    },
  ];

  let cfg = $state<any>(null);
  let message = $state('');
  let error = $state('');
  let busy = $state(false);
  let s = $derived(status());

  onMount(async () => {
    refreshStatus();
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) cfg = r.json;
  });

  const chasing = (a: (typeof AWARDS)[number]) => cfg && (cfg[a.newField] || cfg[a.unconfField]);

  /// The selector: on → the New level (the ? half stays a separate choice,
  /// matching the DXCC convention); off → both, so nothing lingers hidden.
  function toggle(a: (typeof AWARDS)[number]) {
    if (chasing(a)) {
      cfg[a.newField] = false;
      cfg[a.unconfField] = false;
    } else {
      cfg[a.newField] = true;
    }
  }

  async function save() {
    busy = true; message = ''; error = '';
    const r = await api('PUT', '/api/config/me/clublog', { ...cfg });
    busy = false;
    if (r.status === 200) {
      message = 'Saved.';
      // Every open rail re-filters immediately — no reload.
      for (const a of AWARDS) setChase(a.id, !!chasing(a));
    } else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  let anyDxcc = $derived(cfg && DXCC_ORDER.some((k) => cfg[DXCC_FIELD[k]]));
</script>

{#if cfg}
  <div class="card">
    <h2>
      DXCC
      <HelpTip label="DXCC">
        <span class="para">
          Which levels your log flags <b>at all</b>. <b>New</b> means never
          worked; <b>?</b> means worked and still not confirmed — the QSL gap
          you close by working it again.
        </span>
        <span class="para">
          The widest of the controls: a level switched off here is never
          assigned, so it disappears from the spots feed <em>and</em> from
          Telegram. The <b>Alerts</b> tab's "ping me for" only narrows what
          this allows, and only for Telegram.
        </span>
      </HelpTip>
    </h2>
    <div class="levels">
      {#each DXCC_ORDER as k (k)}
        <label data-level={k}>
          <input type="checkbox" bind:checked={cfg[DXCC_FIELD[k]]} />
          <span class="level-dot"></span>{DXCC_LABEL[k]}
        </label>
      {/each}
    </div>
    {#if !anyDxcc}
      <p class="warn">No levels ticked — nothing will ever be flagged, on screen or in Telegram.</p>
    {/if}
  </div>

  <div class="card">
    <h2>
      Other awards
      <HelpTip label="Other awards">
        <span class="para">
          Ticking an award is what makes its levels exist: an award left
          unticked adds no chips, no ladder rows and no pings anywhere —
          the app stays exactly as compact as before.
        </span>
        <span class="para">
          <b>Worked</b> comes from your ClubLog log. <b>Confirmed</b> needs
          the login on <b>LoTW account</b> — ClubLog's export carries no
          state, island or QSL detail, and your LoTW QSL report is what
          does.
        </span>
      </HelpTip>
    </h2>

    {#each AWARDS as a (a.id)}
      <div class="award">
        <label class="chase">
          <input type="checkbox" checked={chasing(a)} onchange={() => toggle(a)} />
          <b>{a.name}</b>
        </label>
        <!-- What the award runs on is a read-once explanation, so it hovers
             rather than sitting under every row: three of these as body text
             pushed the ticks themselves down the page. -->
        <HelpTip label={a.name}>{a.what}</HelpTip>
        {#if chasing(a)}
          <div class="pair">
            <label data-level={a.newKey}>
              <input type="checkbox" bind:checked={cfg[a.newField]} />
              <span class="level-dot"></span>{a.newLabel}
            </label>
            <label data-level={a.unconfKey}>
              <input type="checkbox" bind:checked={cfg[a.unconfField]} />
              <span class="level-dot"></span>{a.unconfLabel}
              <span class="hint">worked, not confirmed</span>
            </label>
          </div>
          {#if a.id === 'was' && s && !s.fcc_calls}
            <p class="warn">
              The FCC call→state table is not on this server yet — State
              levels stay quiet until an admin downloads it under
              <b>Server › Reference data</b>.
            </p>
          {/if}
          {#if a.id === 'iota' && s && !s.iota_groups}
            <p class="hint">
              The IOTA directory has not been downloaded here yet (Server ›
              Reference data) — references from spots pass unvalidated
              until it is.
            </p>
          {/if}
        {/if}
      </div>
    {/each}

    <div class="actions">
      <button class="primary" onclick={save} disabled={busy}>Save</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
{:else}
  <div class="card"><p class="hint">Loading…</p></div>
{/if}

<style>
  .levels {
    display: grid;
    grid-auto-flow: column;
    grid-template-rows: repeat(4, auto);
    grid-template-columns: repeat(2, minmax(8.5rem, 1fr));
    gap: 0.35rem 1rem;
    max-width: 24rem;
  }

  .levels label,
  .pair label {
    gap: 0.45rem;
  }

  @media (max-width: 30rem) {
    .levels {
      grid-auto-flow: row;
      grid-template-columns: 1fr;
      grid-template-rows: none;
    }
  }

  /* One award, one block: the chase tick, what it runs on, then its pair —
     revealed only while chased, which is the page practising the clutter
     rule it exists to enforce. */
  .award {
    padding: 0.7rem 0;
    border-top: 1px solid var(--border);
  }

  .pair {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem 1.5rem;
    margin: 0.35rem 0 0 1.55rem;
  }

  .pair .hint {
    margin-left: 0.35rem;
  }

  .warn {
    color: var(--warn);
    font-size: var(--fs-hint);
    max-width: 38rem;
  }

  p {
    margin: 0.5rem 0 0;
  }
</style>
