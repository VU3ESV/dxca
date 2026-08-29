<script lang="ts">
  // The "?" that holds a screen's explanatory prose.
  //
  // Ported from Meridian's `web-ui/default/src/lib/HelpTip.svelte`, like the
  // rest of this app's look (see the licence note at the top of app.css) —
  // minus the help-index fetch and the "Learn more" drawer, which DXCA has no
  // backend for. The text lives in the CALLER's markup as a snippet instead,
  // so a tip can still carry <b>, <code> and the odd {#if}, and nothing has
  // to be re-typed into a registry that would then drift from the screen.
  //
  // Why the prose moved in here at all: every settings card had grown a
  // paragraph under each field. They are read once and then never again, and
  // in the meantime they were pushing the controls that ARE used every day
  // off the bottom of the screen.
  //
  // Opens on hover and on click. Hover is the cheap read; the click PINS it,
  // so a tip you are still reading survives the pointer wandering off — which
  // is the whole reason Meridian's is click-only.
  import type { Snippet } from 'svelte';

  let { label, children }: { label: string; children: Snippet } = $props();

  let pinned = $state(false);
  let hovering = $state(false);
  // Set when a click closes a tip while the pointer is STILL on the icon:
  // without it the hover that is already true would immediately reopen what
  // the click just shut, and the icon would look dead. Cleared on the way
  // out, so the next hover opens normally. Only ever set while `hovering`,
  // or a tip dismissed with Escape from the keyboard would refuse to open
  // again the next time the pointer arrived.
  let dismissed = $state(false);
  let open = $derived(pinned || (hovering && !dismissed));

  let wrapper = $state<HTMLElement | null>(null);
  let pop = $state<HTMLElement | null>(null);

  // The popover is centred on its icon, which hangs off the screen when the
  // icon sits near an edge — a tip on a `.settings-form` label is close to
  // the left margin on a narrow window. Measure and nudge it back inside.
  const EDGE_MARGIN = 8;
  function reposition() {
    if (!open || !pop) return;
    // Re-centre before measuring, or a second pass would compound the shift.
    pop.style.setProperty('--nudge', '0px');
    const r = pop.getBoundingClientRect();
    const shift =
      r.left < EDGE_MARGIN
        ? EDGE_MARGIN - r.left
        : r.right > window.innerWidth - EDGE_MARGIN
          ? window.innerWidth - EDGE_MARGIN - r.right
          : 0;
    if (shift) pop.style.setProperty('--nudge', `${shift}px`);
  }
  // On open, and again on resize: the viewport edge it was nudged off can
  // move while the popover is still up.
  $effect(reposition);

  // Toggles the PIN, not the openness. Toggling openness looked equivalent and
  // was not: with a mouse you are always hovering by the time you click, so
  // `open` is already true and every click took the closing branch — the pin
  // was unreachable with a pointer, and clicking a tip you were reading shut
  // it. Keyboard activation lands here with `hovering` false and reads the
  // same way.
  function toggle() {
    if (pinned) {
      pinned = false;
      dismissed = hovering;
    } else {
      pinned = true;
      dismissed = false;
    }
  }
  function enter() {
    hovering = true;
  }
  function leave() {
    hovering = false;
    dismissed = false;
  }
  function onWindowClick(e: MouseEvent) {
    if (pinned && wrapper && !wrapper.contains(e.target as Node)) pinned = false;
  }
  function onWindowKey(e: KeyboardEvent) {
    if (open && e.key === 'Escape') {
      pinned = false;
      dismissed = hovering;
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKey} onresize={reposition} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="help-tip" bind:this={wrapper} onmouseenter={enter} onmouseleave={leave}>
  <button
    type="button"
    class="help-icon"
    class:open
    aria-label="About {label}"
    aria-expanded={open}
    onclick={toggle}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  </button>
  {#if open}
    <span class="help-pop" role="dialog" aria-label={label} bind:this={pop}>
      <span class="help-pop-title">{label}</span>
      <span class="help-pop-body">{@render children()}</span>
    </span>
  {/if}
</span>

<style>
  .help-tip {
    position: relative;
    display: inline-flex;
    vertical-align: middle;
    /* The icon rides beside a heading or a label; the gap belongs to the tip
       so no caller has to remember it. */
    margin-left: 0.3rem;
  }

  .help-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 1px;
    margin: 0;
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: help;
    line-height: 0;
    border-radius: 50%;
  }

  .help-icon:hover,
  .help-icon.open {
    color: var(--accent);
  }

  .help-icon:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* Everything below is a phrasing-content box on purpose: a tip lives inside
     an <h2> or a <span class="label">, and a <div>/<p> in there is invalid
     HTML. `display: block` buys the layout back. */
  .help-pop {
    display: block;
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    /* `--nudge` is set on open when centring would push it off a screen edge. */
    transform: translateX(calc(-50% + var(--nudge, 0px)));
    z-index: 40;
    width: 17rem;
    max-width: 78vw;
    text-align: left;
    /* Opaque: it overlays the card it explains. */
    background: Canvas;
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 0.6rem 0.75rem;
    box-shadow: 0 6px 20px color-mix(in srgb, CanvasText 18%, transparent);
    cursor: default;
    /* A tip lands inside whatever typography its host set, and prose must not
       inherit any of it: headings are uppercase and letter-spaced, a status
       pill and every feed table cell are `white-space: nowrap`. The nowrap is
       the one that actually breaks the box — the paragraph refuses to wrap and
       runs straight out through the right border, which is what the Server
       health tip did in the app header. Reset the lot here rather than
       per-caller; a tip is a new context, not a continuation of its anchor. */
    text-transform: none;
    letter-spacing: normal;
    font-weight: 400;
    font-style: normal;
    white-space: normal;
    text-align: left;
  }

  /* Bridges the 6px gap under the icon so the pointer never leaves the
     wrapper's subtree on its way into the popover — without it the tip
     flickers shut the moment you reach for the text. */
  .help-pop::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: -8px;
    height: 8px;
  }

  .help-pop-title {
    display: block;
    font-size: var(--fs-hint);
    font-weight: 600;
    margin-bottom: 0.25rem;
    color: CanvasText;
  }

  .help-pop-body {
    display: block;
    font-size: var(--fs-hint);
    line-height: 1.5;
    color: var(--muted);
  }

  /* A tip that carries more than one paragraph marks them up as `.para`
     spans. The rule has to be :global — snippet content is compiled into the
     CALLING component, so it never wears this component's scope hash. */
  .help-pop-body :global(.para) {
    display: block;
  }

  .help-pop-body :global(.para + .para) {
    margin-top: 0.5rem;
  }

  /* A `<b>` inside the muted body should lift off it, not sit at the same
     weight and colour as everything around it. */
  .help-pop-body :global(b) {
    color: CanvasText;
    font-weight: 600;
  }

  .help-pop-body :global(code) {
    font-family: var(--mono);
    font-size: 0.95em;
  }
</style>
