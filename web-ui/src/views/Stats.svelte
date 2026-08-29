<script lang="ts">
  // What the feed is made of: one headline number and three breakdowns.
  //
  // **Bars, not pie charts.** The job here is magnitude comparison — "which
  // band carries most of my spots" — across up to fifteen categories with
  // long names like "UberSDR CWskim". A pie with fifteen slices is
  // unreadable and cannot hold its own labels; horizontal bars compare
  // exactly, sort meaningfully, and leave room for the text.
  //
  // Each chart is a SINGLE series, so there is no legend: the heading names
  // it, and colour carries no meaning beyond "this is the bar". Fifteen
  // different hues for fifteen bands would be encoding identity that the
  // labels already carry, and would fail on colour-vision grounds for
  // nothing in return.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';

  let stats = $state<any>(null);
  let error = $state('');

  async function load() {
    const r = await api('GET', '/api/spot-stats');
    if (r.status === 200) {
      stats = r.json;
      error = '';
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  onMount(() => {
    load();
    // The ring turns over in under an hour on a busy feed, so a static
    // snapshot goes stale while you look at it.
    const t = setInterval(load, 15000);
    return () => clearInterval(t);
  });

  /// Longest bar in a group defines full width, so each chart uses its own
  /// scale. Comparing across the three would be meaningless — they count
  /// the same spots three different ways.
  const peak = (rows: any[]) => Math.max(1, ...rows.map((r) => r.count));

  const pct = (n: number, total: number) =>
    total > 0 ? ((n / total) * 100).toFixed(1) : '0.0';

  /// Returns the whole phrase, "about" included, because the short case
  /// cannot take that hedge: a freshly started instance reading "about 0 min
  /// of feed" states a measurement where there isn't one yet, and "about
  /// under a minute" is not English.
  function span(secs: number): string {
    if (!secs) return '';
    const m = Math.round(secs / 60);
    if (m < 1) return 'under a minute';
    if (m < 90) return `about ${m} min`;
    return `about ${(m / 60).toFixed(1)} hours`;
  }
</script>

<div class="page narrow">
  {#if error}
    <div class="card"><p class="err">{error}</p></div>
  {:else if !stats}
    <div class="card"><p class="hint">Counting…</p></div>
  {:else}
    <!-- The headline is a number, not a chart: one value has no shape to
         plot, and a gauge or a single-slice pie would be decoration. -->
    <div class="card total-card">
      <div class="total">
        <span class="num">{stats.total.toLocaleString()}</span>
        <span class="cap">spots held</span>
      </div>
      <p class="hint">
        <!-- {' '} rather than a literal space: Svelte trims the leading
             whitespace of a block's content wherever it is written, so both
             "memory{#if}\n — about" and "memory{#if} — about" render as
             "memory— about". An explicit expression is the only form that
             survives, and the space cannot move outside the block — it
             would leave "memory ." on an instance with no span yet. -->
        Everything DXCA currently has in memory{#if stats.span_secs}{' '}—
          {span(stats.span_secs)} of feed{/if}. The ring keeps the most
        recent spots and discards the oldest, so this is a window, not a
        running total since startup.
      </p>
    </div>

    {#each [{ title: 'By band', hue: 'band', rows: stats.bands, note: 'In band order, not by count — this reads as a band plan.' }, { title: 'By mode', hue: 'mode', rows: stats.modes, note: 'As reported by the decoder or the spot comment, so FT8 and FT4 stay apart.' }, { title: 'By source', hue: 'source', rows: stats.sources, note: 'The feed that carried the spot — a decoder here, or the cluster node that relayed it.' }] as group (group.title)}
      <div class="card" data-hue={group.hue}>
        <h2>{group.title}</h2>
        <p class="hint sub">{group.note}</p>
        {#if !group.rows.length}
          <p class="hint">Nothing yet.</p>
        {:else}
          <div class="bars">
            {#each group.rows as row (row.key)}
              <div class="row">
                <span
                  class="key"
                  title="{row.key}: {row.count} spots ({pct(row.count, stats.total)}% of the ring)"
                  >{row.key}</span
                >
                <span class="track">
                  <span
                    class="bar"
                    style="width: {(row.count / peak(group.rows)) * 100}%"
                  ></span>
                </span>
                <span class="val mono">{row.count.toLocaleString()}</span>
                <span class="share mono">{pct(row.count, stats.total)}%</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<style>
  .total-card {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .total {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  /* The hero number carries the weight; its caption stays recessive so the
     eye lands on the value first. */
  .total .num {
    font-size: 2.4rem;
    font-weight: 700;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }

  .total .cap {
    color: var(--muted);
    font-size: 0.85rem;
  }

  /* One grid for the whole chart rather than one per row, and a FIXED label
     column so all three charts share it.

     `max-content` sized each chart to its own longest name, which read fine
     one chart at a time and ragged down the page: "160M" and "FT8" and
     "DB0SUE" each started their bars at a different x, so the three charts
     never lined up. Looking at the whole page is what showed it — the
     charts are stacked, so they are compared whether or not they were
     meant to be.

     The width is the answer to "what is the longest name here", and the
     labels WRAP rather than truncate when something exceeds it. Truncating
     a node's name in a chart about which node carried what would be exactly
     the wrong thing to lose; a two-line label costs one row of height and
     keeps every character. */
  .bars {
    display: grid;
    grid-template-columns: 7.5rem 1fr max-content max-content;
    align-items: center;
    /* 2px between fills, so adjacent bars read as separate marks rather
       than one block. */
    row-gap: 2px;
    column-gap: 0.6rem;
    font-size: 0.82rem;
  }

  .row {
    display: contents;
  }

  /* Wraps rather than overflows or truncates — see .bars above. */
  .key {
    color: var(--fg);
    overflow-wrap: anywhere;
  }

  /* The track is the recessive frame the bar sits in — it shows the scale
     without competing with the data. */
  .track {
    background: color-mix(in srgb, CanvasText 7%, Canvas);
    border-radius: 3px;
    height: 14px;
    overflow: hidden;
  }

  /* One hue per CHART, not per bar.
     
     Colour here answers "which breakdown am I reading", which is real if
     modest information; fifteen hues for fifteen bands would encode
     identity the labels already carry, and would fail on colour-vision
     grounds for nothing in return.

     None of these three is a colour DXCA already means something by. Red,
     orange and yellow are the DXCC/Slot/Mode alert levels, blue is New
     Band and the app accent, and green is the `ok` status — an orange bar
     in here would read as "New Slot". Teal, violet and pink are what is
     left, and each was run through the palette validator rather than
     picked by eye: light steps clear the chroma floor and 3:1 contrast on
     a light surface, and dark mode gets its OWN steps (a flipped palette
     lands outside the lightness band, which is exactly what the validator
     is for). */
  [data-hue='band'] {
    --series: light-dark(#0891b2, #22a7b3);
  }

  [data-hue='mode'] {
    --series: light-dark(#6639ba, #a371f7);
  }

  [data-hue='source'] {
    --series: light-dark(#bf3989, #db61a2);
  }

  /* Thin mark, rounded at the data end only: the baseline end stays square
     because it is anchored to zero, not a value. */
  .bar {
    display: block;
    height: 100%;
    background: var(--series, var(--accent));
    border-radius: 0 4px 4px 0;
    min-width: 2px;
  }

  /* The heading wears its chart's hue as a small marker, so the colour is
     tied to a name rather than floating free. The heading TEXT stays in
     the ordinary ink — text never wears the series colour. */
  .card h2::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 0.45rem;
    border-radius: 2px;
    background: var(--series, var(--accent));
    vertical-align: middle;
  }

  /* Values wear text tokens, never the series colour — identity is carried
     by the label beside them. */
  .val {
    text-align: right;
    color: var(--fg);
    font-variant-numeric: tabular-nums;
  }

  .share {
    text-align: right;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .sub {
    margin: -0.35rem 0 0.7rem;
    line-height: 1.45;
  }
</style>
