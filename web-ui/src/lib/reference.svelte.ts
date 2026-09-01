// The band / mode-class / alert-level vocabularies, fetched once from
// GET /api/reference and shared by every screen that builds a picker.
//
// Served rather than hardcoded here on purpose: the band table, the award
// buckets and the level ladder all live in dxca-core, and a second copy in
// TypeScript would drift the moment a band or a level is added. One fetch per
// page load, cached for the session — these change only when the server binary
// does.

import { api } from './api';

export interface Option {
  key: string;
  label: string;
  /// Levels only: which award this level belongs to ('vucc' | 'was' |
  /// 'iota'), null/absent for the classic DXCC eight. What `lib/chase`
  /// filters level lists on.
  award?: string | null;
}

const state = $state<{
  bands: Option[];
  modes: Option[];
  levels: Option[];
  loaded: boolean;
}>({ bands: [], modes: [], levels: [], loaded: false });

let inflight: Promise<void> | null = null;

/** Fetch once; concurrent callers share the same request. */
export function loadReference(): Promise<void> {
  if (state.loaded) return Promise.resolve();
  if (inflight) return inflight;
  inflight = (async () => {
    const r = await api('GET', '/api/reference');
    if (r.status === 200 && r.json) {
      // Bands and modes come back as bare strings; the level list already
      // carries its own human label from AlertLevel::label().
      state.bands = (r.json.bands ?? []).map((b: string) => ({ key: b, label: b }));
      // "PHONE"/"DATA" shout in a chip row, but CW is an acronym and must not
      // be title-cased into "Cw". Anything the server adds later falls back to
      // its own key rather than being mangled.
      const MODE_LABEL: Record<string, string> = { CW: 'CW', PHONE: 'Phone', DATA: 'Data' };
      state.modes = (r.json.modes ?? []).map((m: string) => ({
        key: m,
        label: MODE_LABEL[m] ?? m,
      }));
      state.levels = r.json.levels ?? [];
      state.loaded = true;
    }
    inflight = null;
  })();
  return inflight;
}

export const bands = () => state.bands;
export const modes = () => state.modes;
export const levels = () => state.levels;
export const referenceLoaded = () => state.loaded;

/** Level key → its human label, for the Alert cell or a chip. */
export function levelLabel(key: string): string {
  return state.levels.find((l) => l.key === key)?.label ?? '';
}

