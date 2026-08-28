// Whether award totals should also count **deleted** DXCC entities.
//
// The ARRL maintains a deleted list — Abu Ail, Blenheim Reef, British North
// Borneo and 59 others. A QSO with one is a real contact and stays in the
// log, but it scores nothing toward current DXCC or the Challenge, so
// totals that include them match no published standing.
//
// **Current-only is the default**, because that is what the ARRL publishes
// and therefore what an operator is comparing against; including the
// deleted list is the deliberate exception, so it is the thing you tick.
//
// Shared across screens rather than owned by one: the station card on Spots
// and the statistics card in My ClubLog show the same numbers, and letting
// them disagree about which entities count would be worse than either
// answer on its own. Persisted per browser, like the spot filters — it is a
// view preference, not an account setting.

const STORE_KEY = 'dxca.awards';

function restore(): boolean {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}').includeDeleted === true;
  } catch {
    return false;
  }
}

const state = $state<{ includeDeleted: boolean }>({ includeDeleted: restore() });

export const awards = {
  get includeDeleted() {
    return state.includeDeleted;
  },
  set includeDeleted(v: boolean) {
    state.includeDeleted = v;
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify({ includeDeleted: v }));
    } catch {
      // Private mode / storage disabled — the toggle still works this session.
    }
  },
};

/// Pick the right pair of totals for the current preference.
///
/// Falls back to the unfiltered set when the server sent no filtered one,
/// which happens when no cty.xml is loaded: without it there is no way to
/// know which entities are deleted. Callers use [`canFilter`] to hide the
/// tickbox entirely in that case, rather than offer a choice that cannot
/// be honoured.
export function pick<T>(all: T, current: T | null | undefined): T {
  return awards.includeDeleted || current == null ? all : current;
}

/// Does the server know which entities are deleted?
export function canFilter(current: unknown): boolean {
  return current != null;
}
