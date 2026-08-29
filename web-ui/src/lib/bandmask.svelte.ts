// The phase-rotation band mask, client side
// (`docs/PHASE-ROTATION-MASK.md`, milestone 3).
//
// The server marks each spot with whether its band is plausibly workable
// from the operator's QTH right now — it never withholds anything. This
// decides what to do about that, and the answer is deliberately mild:
// **dim, never hide**.
//
// The whole feature is governed by one asymmetry: the cost of concealing a
// workable rare one is far higher than the cost of showing an unworkable
// one. An operator who misses a New DXCC because software decided it was
// daytime will not forgive it, and will stop trusting the rest of the
// screen. So this recedes spots rather than removing them, it is off until
// switched on, and it never touches the rarest alert level.

const STORE_KEY = 'dxca.bandmask';

function restore(): boolean {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}').on === true;
  } catch {
    return false;
  }
}

const state = $state<{ on: boolean }>({ on: restore() });

export const bandMask = {
  get on() {
    return state.on;
  },
  set on(v: boolean) {
    state.on = v;
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify({ on: v }));
    } catch {
      // Private mode / storage disabled — it still works this session.
    }
  },
};

/// Alert levels the mask never touches, rarest first.
///
/// Design doc §4: "never mask at or above this level", floored at New DXCC.
/// That level is the one spot worth breaking every rule for — it may be the
/// only time that entity appears all year, the operator can judge
/// propagation better than this model can, and being wrong there is the
/// unforgivable case.
///
/// It is a list rather than a bare string because milestone 4 makes the
/// floor a setting; until then the default is the whole of it. Anything
/// below New DXCC — including New Band — does get dimmed, which is the
/// point: a New Band flag on 160m at local noon is exactly the
/// interruption the operator can do nothing about.
const NEVER_MASK = ['newDXCC'];

/// Should this spot be shown receded?
///
/// False whenever anything is missing — no locator (the server sends no
/// `band_open` at all), no classification, the mask switched off. Silence
/// from the server means "no opinion", never "hide it".
export function masked(spot: any): boolean {
  if (!state.on) return false;
  if (spot?.band_open !== false) return false;
  return !NEVER_MASK.includes(spot?.alert);
}
