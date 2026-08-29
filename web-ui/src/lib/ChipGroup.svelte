<script lang="ts">
  // A pick-any-subset row of chips, with All / None shortcuts.
  //
  // Deliberately shared by the Spots screen (an ephemeral display narrowing)
  // and My Alerts (a persisted Telegram narrowing): the two mean different
  // things but they ASK the same question, and the band list in particular is
  // long enough that two hand-rolled versions would drift.
  //
  // The empty set means EVERYTHING, matching the server's own
  // `notify_bands` / `notify_modes` convention — so a fresh account is not
  // silent, and "All" is stored as `[]` rather than as all fifteen bands
  // (which would silently stop tracking the band list if it ever grew).

  let {
    label,
    options,
    selected = $bindable(),
    // Optional per-option colour token, used by the level picker so a chip
    // wears the same hue as the row it will show.
    levelKeys = false,
    // Label ABOVE the chips rather than beside them. For the Spots filter
    // rail, which is 12rem wide: inline, the label eats a third of the row and
    // the chips wrap after two.
    stacked = false,
  }: {
    label: string;
    options: { key: string; label: string }[];
    selected: Set<string>;
    levelKeys?: boolean;
    stacked?: boolean;
  } = $props();

  function toggle(key: string) {
    const next = new Set(selected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    selected = next;
  }

  const allOn = $derived(selected.size === 0);
</script>

<div class="chipgroup" class:stacked>
  <span class="grouplabel">{label}</span>
  <button
    class="filter-chip all"
    class:on={allOn}
    aria-pressed={allOn}
    title="Everything — the same as picking none"
    onclick={() => (selected = new Set())}
  >All</button>
  {#each options as o (o.key)}
    <button
      class="filter-chip"
      class:on={selected.has(o.key)}
      aria-pressed={selected.has(o.key)}
      data-level={levelKeys ? o.key : undefined}
      onclick={() => toggle(o.key)}
    >
      {#if levelKeys}<span class="level-dot"></span>{/if}
      {o.label}
    </button>
  {/each}
</div>

<style>
  .chipgroup {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem;
  }

  .grouplabel {
    color: var(--muted);
    font-size: 0.8rem;
    margin-right: 0.15rem;
  }

  /* Stacked: the label becomes a rail heading and takes the whole first line,
     so the chips below it get the full 12rem to wrap in. Same vocabulary as
     `.rail-head` in app.css, because in the rail that is exactly what it is. */
  .chipgroup.stacked {
    gap: 0.25rem;
  }

  .chipgroup.stacked .grouplabel {
    flex-basis: 100%;
    margin-right: 0;
    font-size: 0.62rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.11em;
  }

  /* Picked = accent ring, not the level's own hue: the dot already says WHICH
     level, so the ring is free to say only "this one is on" — and a selected
     chip then looks identical whichever level wears it. */
  .filter-chip.on {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  /* "All" is the resting state, not a selection — it reads as a quiet
     default rather than competing with a real pick. */
  .filter-chip.all.on {
    border-color: var(--border);
    color: CanvasText;
    font-weight: 500;
  }
</style>
