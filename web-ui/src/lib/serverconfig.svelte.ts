// The server-wide config (`config/dxca.toml`), held once for the four Settings
// pages that edit slices of it.
//
// The API is all-or-nothing: `PUT /api/config/global` replaces sources, nodes,
// destinations and the ClubLog API key together, so a page that sent only its
// own slice would wipe the other three. Rather than repeat "load the whole
// object, write the whole object back" in four components — and rely on four
// authors remembering it — the object lives here and every page edits the same
// one. Switching rail pages then also keeps unsaved edits, which four
// independent copies would have thrown away on every click.

import { api } from './api';

export const server = $state<{
  cfg: any;
  loaded: boolean;
  busy: boolean;
  message: string;
  error: string;
}>({ cfg: null, loaded: false, busy: false, message: '', error: '' });

/** Load once per session. Repeated calls are cheap no-ops, so every page can
 * call it on mount without coordinating. Pass `true` after a refresh that the
 * server may have changed underneath us (a cty download rewrites timestamps). */
export async function loadServerConfig(force = false): Promise<void> {
  if (server.loaded && !force) return;
  const r = await api('GET', '/api/config/global');
  if (r.status === 200) {
    server.cfg = r.json;
    server.loaded = true;
  } else {
    server.error = r.json?.error ?? `HTTP ${r.status}`;
  }
}

/** Apply and persist. Sends every field the endpoint owns, always. */
export async function saveServerConfig(): Promise<void> {
  if (!server.cfg) return;
  server.busy = true;
  server.message = '';
  server.error = '';
  const r = await api('PUT', '/api/config/global', {
    udp_sources: server.cfg.udp_sources,
    cluster_nodes: server.cfg.cluster_nodes,
    broadcast_destinations: server.cfg.broadcast_destinations,
    // Sent every save. The server treats an ABSENT field as "leave as-is" and
    // an empty string as a deliberate clear, so always sending the current
    // value keeps the Reference data box the source of truth.
    clublog_api_key: server.cfg.clublog_api_key ?? '',
  });
  server.busy = false;
  if (r.status === 200) server.message = 'Applied live and saved to config/dxca.toml.';
  else server.error = r.json?.error ?? `HTTP ${r.status}`;
}

/** Drop a row from one of the three lists. */
export const drop = (list: any[], i: number) => list.filter((_, idx) => idx !== i);

/// A source name is a COLUMN WIDTH in the spots feed: the table is fixed-layout
/// now, and the Source column is sized to the longest name an operator has
/// configured. Fourteen characters is what "UberSDR CWskim" needs and what the
/// column was measured against — past that the name would clip, which for the
/// one column whose values the operator chose themselves is a poor trade
/// against simply choosing a shorter name.
export const SOURCE_NAME_MAX = 14;
