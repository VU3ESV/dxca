// Appearance — the operator's light / dark pick, or neither.
//
// LICENSE: this module is derived from the Meridian project's
// `web-ui/default/src/lib/theme.svelte.ts` — © the Meridian authors (Basil
// Thomas W6BT, Vinod VU3ESV, Ram VU3RDD), Apache-2.0 — and remains under
// Apache-2.0 (see LICENSE-APACHE at the repo root), unlike the rest of this
// MIT repo. DXCA's modifications are marked `// DXCA:`.
//
// DXCA: the storage key is `dxca.theme`, and the labels are plain English
// rather than i18n lookups (DXCA has no i18n layer).
//
// The whole UI is built on CSS system colors (`Canvas` / `CanvasText`) plus
// `color-scheme`, so it already follows the OS with no work at all. What CSS
// cannot express is an explicit OVERRIDE, and that is this module's whole job:
// pinning `color-scheme` on <html> re-resolves every system color, while
// leaving it unset hands the choice back to the OS.
//
// Three preferences, not two. `auto` is a state the operator can CHOOSE and
// return to, not merely the absence of a choice — without it the first click
// pins the appearance forever, and a shack machine that switches itself at
// sunset stops taking the dashboard with it. It is stored explicitly for the
// same reason: an unset slot and a stored `auto` resolve alike today, but only
// the stored one still means "the operator wants the OS to decide" if the
// default ever moves.

/** An appearance actually on screen. */
export type Theme = 'light' | 'dark';

/** What the operator picked — `auto` defers to the OS. */
export type ThemePref = Theme | 'auto';

/** Menu order, and the vocabulary a stored value is validated against. */
export const THEME_PREFS: readonly ThemePref[] = ['auto', 'light', 'dark'];

/** Matches the pre-render script in index.html, which reads the same slot to
 *  pin the appearance before first paint. Change one, change both. */
const STORAGE_KEY = 'dxca.theme';

const LABELS: Record<ThemePref, string> = {
  auto: 'Auto',
  light: 'Light',
  dark: 'Dark',
};

/** Human name for a preference — used by the switcher's menu and title. */
export function themeLabel(pref: ThemePref): string {
  return LABELS[pref];
}

function systemTheme(): Theme {
  try {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

function stored(): ThemePref | null {
  try {
    const s = localStorage.getItem(STORAGE_KEY);
    return THEME_PREFS.includes(s as ThemePref) ? (s as ThemePref) : null;
  } catch {
    // Private mode, storage disabled, or no localStorage at all.
    return null;
  }
}

// The OS theme is tracked whatever the pick is, so the menu can name what
// `auto` resolves to right now even while the operator sits on a pinned one.
const state = $state<{ pref: ThemePref; system: Theme }>({
  pref: stored() ?? 'auto',
  system: systemTheme(),
});

function apply() {
  try {
    const el = document.documentElement;
    if (state.pref === 'auto') el.style.removeProperty('color-scheme');
    else el.style.colorScheme = state.pref;
  } catch {
    // no document (shouldn't happen in the browser SPA)
  }
}

/** The operator's pick, `auto` included (reactive). */
export function themePref(): ThemePref {
  return state.pref;
}

/** The appearance on screen (reactive): the pick, or the OS under `auto`. */
export function currentTheme(): Theme {
  return state.pref === 'auto' ? state.system : state.pref;
}

/** What the OS is asking for (reactive) — i.e. what `auto` resolves to. This
 *  is NOT `currentTheme()` while a pick is pinned: the menu has to name where
 *  Auto would land, which is the one thing a pinned screen cannot show. */
export function osTheme(): Theme {
  return state.system;
}

/** Pick an appearance and remember it across reloads. */
export function setThemePref(pref: ThemePref) {
  state.pref = pref;
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch {
    // Never worth failing over: the pick still applies for this session.
  }
  apply();
}

apply();

// Follow the OS live, so `auto` turns dark at sunset without a reload.
try {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
    state.system = e.matches ? 'dark' : 'light';
  });
} catch {
  // matchMedia unavailable — nothing to track.
}
