<script lang="ts">
  // The header's appearance picker. The trigger wears app.css's shared
  // `.gear` icon-button shape and the menu its shared `.popup-menu`, so the
  // header cannot grow a second dropdown idiom.
  //
  // LICENSE: derived from the Meridian project's
  // `web-ui/default/src/lib/ThemeSwitcher.svelte` — © the Meridian authors
  // (Basil Thomas W6BT, Vinod VU3ESV, Ram VU3RDD), Apache-2.0 — and remains
  // under Apache-2.0 (see LICENSE-APACHE at the repo root), unlike the rest
  // of this MIT repo. DXCA's modifications are marked `// DXCA:`.
  //
  // DXCA: labels come from `themeLabel()` instead of the i18n store, and the
  // trigger reuses the shared `.gear` class rather than a private `.theme-btn`.
  import { osTheme, setThemePref, themeLabel, themePref, THEME_PREFS, type ThemePref } from './theme.svelte';

  let open = $state(false);
  let widget: HTMLDivElement | undefined = $state();

  // Any click outside closes the menu. Our own button's click bubbles here
  // too, so the containment check is what leaves the trigger working.
  function dismiss(e: MouseEvent) {
    if (open && widget && !widget.contains(e.target as Node)) open = false;
  }

  // The trigger names the PICK, not the appearance: the appearance is already
  // on screen, and "auto" is the one state nothing else on the page reveals.
  const label = $derived(`Theme: ${themeLabel(themePref())}`);
</script>

<svelte:window
  onclick={dismiss}
  onkeydown={(e) => {
    if (e.key === 'Escape') open = false;
  }}
/>

{#snippet icon(pref: ThemePref)}
  {#if pref === 'light'}
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </svg>
  {:else if pref === 'dark'}
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
    </svg>
  {:else}
    <!-- Half-filled disc: the OS decides which half you get. -->
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 3a9 9 0 0 0 0 18z" fill="currentColor" stroke="none" />
    </svg>
  {/if}
{/snippet}

<div class="theme-widget" bind:this={widget}>
  <button
    class="gear"
    class:menu-open={open}
    title={label}
    aria-label={label}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    {@render icon(themePref())}
  </button>
  {#if open}
    <div class="popup-menu" role="menu">
      {#each THEME_PREFS as pref (pref)}
        <button
          class="popup-item"
          class:active={pref === themePref()}
          role="menuitemradio"
          aria-checked={pref === themePref()}
          onclick={() => {
            setThemePref(pref);
            open = false;
          }}
        >
          {@render icon(pref)}
          <span>{themeLabel(pref)}</span>
          <!-- Where Auto lands right now. It names the OS, not the screen:
               under a pinned pick those differ, and that is exactly when the
               operator cannot see the answer anywhere else. -->
          {#if pref === 'auto'}
            <span class="resolved">{themeLabel(osTheme())}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .theme-widget {
    position: relative;
    display: flex;
  }

  /* The OS's current answer, trailing the Auto row. */
  .resolved {
    margin-left: auto;
    padding-left: 0.75rem;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>
