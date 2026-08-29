<script lang="ts">
  // The Spots screen's filters, moved off the vertical axis.
  //
  // They used to be five stacked rows above the feed — sources, search + ticks,
  // alerts, modes, bands — which together with the station card and the four
  // status boxes put the first spot 615px down a 1010px window. Sideways, they
  // cost width the feed can spare and give back height it cannot.
  //
  // NOTHING IS HIDDEN AT REST. That is the whole reason this is a rail rather
  // than a row of dropdowns: the house rule is that a narrowing which changes
  // the screen without saying so is indistinguishable from a feed going quiet.
  // When the rail IS collapsed, the spine carries a count of how many
  // narrowings are live — so a folded rail still cannot lie about the feed.
  import type { Snippet } from 'svelte';

  let {
    activeCount,
    children,
  }: { activeCount: number; children: Snippet } = $props();

  const STORE_KEY = 'dxca.filterrail';

  // A view preference, per browser, exactly like the spot filters themselves —
  // it must survive a reload, and it is nobody else's business on the server.
  function load(): boolean {
    try {
      const raw = localStorage.getItem(STORE_KEY);
      return raw === null ? true : raw === 'open';
    } catch {
      return true;
    }
  }

  let pref = $state(load());
  /// True while the window is too narrow to give the rail its column. Driven
  /// by matchMedia rather than an `innerWidth` binding so the flip is a single
  /// event, not a resize storm.
  let narrow = $state(false);
  /// A manual toggle taken since the last breakpoint flip. Held apart from
  /// `pref` so opening the rail on a phone does not rewrite the preference the
  /// desktop window will be restored with — and cleared on every flip, so the
  /// breakpoint gets to state its default again.
  let override = $state<boolean | null>(null);

  let open = $derived(override ?? (narrow ? false : pref));

  $effect(() => {
    const mq = window.matchMedia('(max-width: 64rem)');
    const sync = () => {
      narrow = mq.matches;
      override = null;
    };
    sync();
    mq.addEventListener('change', sync);
    return () => mq.removeEventListener('change', sync);
  });

  function toggle() {
    if (narrow) {
      override = !open;
    } else {
      pref = !open;
      override = null;
      try {
        localStorage.setItem(STORE_KEY, pref ? 'open' : 'shut');
      } catch {
        // Private mode / storage disabled — the toggle still works this session.
      }
    }
  }
</script>

<div class="filter-rail" class:shut={!open}>
  {#if open}
    <div class="rail-top">
      <span class="rail-head">Filters</span>
      <button
        class="collapse"
        type="button"
        onclick={toggle}
        title="Collapse the filters"
        aria-label="Collapse the filters"
        aria-expanded="true">‹</button
      >
    </div>
    {@render children()}
  {:else}
    <button
      class="spine"
      type="button"
      onclick={toggle}
      title={activeCount
        ? `Show the filters — ${activeCount} narrowing${activeCount === 1 ? '' : 's'} active`
        : 'Show the filters'}
      aria-label="Show the filters"
      aria-expanded="false"
    >
      <span class="chev">›</span>
      {#if activeCount}<span class="badge">{activeCount}</span>{/if}
      <span class="vert">Filters</span>
    </button>
  {/if}
</div>

<style>
  /* A fixed column, not a content-sized one: the chip groups inside must wrap
     to a known width, and the feed beside it must not resize every time a
     band chip appears. */
  .filter-rail {
    width: 12rem;
    position: sticky;
    top: 0;
    border-right: 1px solid var(--border);
    padding: 0.9rem 0.7rem;
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
    max-height: 100vh;
    overflow-y: auto;
  }

  /* Collapsed, the rail is a spine — just wide enough for the chevron, the
     count and the word turned on its side. */
  .filter-rail.shut {
    width: auto;
    padding: 0.9rem 0.2rem;
    align-items: center;
    overflow: visible;
  }

  .rail-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
  }

  .rail-head {
    font-size: 0.62rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.11em;
    color: var(--muted);
  }

  .collapse,
  .spine {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
    padding: 0 0.35rem;
    line-height: 1.5;
  }

  .collapse:hover,
  .spine:hover {
    color: var(--accent);
    border-color: var(--accent);
  }

  .spine {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.45rem;
    padding: 0.3rem 0.2rem 0.6rem;
  }

  .spine .chev {
    font-size: 0.85rem;
  }

  /* The promise the collapse has to keep: a folded rail still says how much
     of the feed is being held back. */
  .badge {
    background: var(--accent);
    color: Canvas;
    border-radius: 999px;
    font-size: 0.62rem;
    font-weight: 700;
    padding: 0.05rem 0.32rem;
    font-variant-numeric: tabular-nums;
  }

  .vert {
    writing-mode: vertical-rl;
    font-size: 0.64rem;
    letter-spacing: 0.11em;
    text-transform: uppercase;
  }
</style>
