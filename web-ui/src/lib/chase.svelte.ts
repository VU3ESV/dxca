// Which awards this account is chasing — the declutter filter.
//
// Chasing is not its own stored flag: an award is chased exactly when
// either of its classifier levels is enabled (the pair on Settings › My
// station › Awards). This store reads that once per session and every
// level list in the app — the Alerts ladder, the Spots chips, the Stats
// card — filters through it, so an award nobody opted into adds no
// control anywhere. The classic DXCC eight always show.
//
// Not logged in (or the fetch fails): nothing is chased and the app looks
// exactly as it did before the awards existed.

import { api } from './api';
import type { Option } from './reference.svelte';

const state = $state<{ chased: Record<string, boolean>; loaded: boolean }>({
  chased: {},
  loaded: false,
});

let inflight: Promise<void> | null = null;

/** Fetch once; concurrent callers share the request. */
export function loadChase(): Promise<void> {
  if (state.loaded) return Promise.resolve();
  if (inflight) return inflight;
  inflight = (async () => {
    const r = await api('GET', '/api/config/me/clublog');
    if (r.status === 200 && r.json) {
      const c = r.json;
      state.chased = {
        vucc: !!(c.alert_new_grid || c.alert_unconf_grid),
        was: !!(c.alert_new_state || c.alert_unconf_state),
        iota: !!(c.alert_new_iota || c.alert_unconf_iota),
      };
    }
    state.loaded = true;
    inflight = null;
  })();
  return inflight;
}

export const chasedAny = () => Object.values(state.chased).some(Boolean);

export const isChased = (award?: string | null) => !award || !!state.chased[award];

/** A level list with unchased awards' levels removed (reactive). */
export function chasedLevels(levels: Option[]): Option[] {
  return levels.filter((l) => isChased(l.award));
}

/** The Awards page reports its save here, so every open rail updates
 *  without a reload. */
export function setChase(award: string, on: boolean) {
  state.chased[award] = on;
}
