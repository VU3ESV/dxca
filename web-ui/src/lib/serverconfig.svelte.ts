// Config held once for the Settings pages that edit slices of it.
//
// TWO endpoints, because they need different permissions:
//
//   * `/api/config/me/feeds`  — THIS account's sources, nodes and outputs.
//     Any logged-in operator; they describe that operator's own station.
//   * `/api/config/global`    — admin only, and it must stay that way: it
//     carries the ClubLog API key and the MQTT broker password.
//
// Both are all-or-nothing: a PUT replaces every list it owns, so a page that
// sent only its own slice would wipe the others. Rather than repeat "load the whole
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
  const r = await api('GET', '/api/config/me/feeds');
  if (r.status === 200) {
    // Named as the pages have always known them, so nothing downstream
    // changed when feeds moved off the global endpoint.
    server.cfg = {
      udp_sources: r.json.udp_sources ?? [],
      cluster_nodes: r.json.cluster_nodes ?? [],
      broadcast_destinations: r.json.destinations ?? [],
      passthrough: r.json.passthrough ?? [],
      is_admin: r.json.is_admin === true,
    };
    // The admin-only half: server settings, the API key, MQTT. A non-admin
    // simply does not get it, and the pages that need it are admin-gated
    // anyway.
    if (server.cfg.is_admin) {
      const g = await api('GET', '/api/config/global');
      if (g.status === 200) {
        // Take ONLY the admin-owned keys. A blind spread would overwrite
        // `broadcast_destinations` — the global endpoint still returns a key
        // of that name — putting the server's passthrough row back into this
        // operator's outputs list, where it is not editable and shows a blank
        // Format because passthrough is not a per-account format.
        server.cfg.read_only = g.json.read_only;
        server.cfg.clublog_api_key = g.json.clublog_api_key;
        server.cfg.cty_last_refresh_unix = g.json.cty_last_refresh_unix;
        server.cfg.lotw_last_refresh_unix = g.json.lotw_last_refresh_unix;
      }
    }
    server.loaded = true;
    server.error = '';
  } else {
    // `api` never throws now, so an unreachable server arrives here as
    // status 0 rather than as a rejected promise that left the page on
    // "Loading…" for ever.
    server.error = r.json?.error ?? `HTTP ${r.status}`;
  }
}

/** Apply and persist. Sends every field the endpoint owns, always. */
export async function saveServerConfig(): Promise<void> {
  if (!server.cfg) return;
  server.busy = true;
  server.message = '';
  server.error = '';
  // This account's feeds first — the part every operator may change.
  const r = await api('PUT', '/api/config/me/feeds', {
    udp_sources: server.cfg.udp_sources,
    cluster_nodes: server.cfg.cluster_nodes,
    destinations: server.cfg.broadcast_destinations,
  });
  if (r.status !== 200) {
    server.busy = false;
    server.error = r.json?.error ?? `HTTP ${r.status}`;
    return;
  }
  // Then the admin-only half, only if this operator owns it. Skipping it for
  // a non-admin is what lets these pages leave the Server group.
  if (server.cfg.is_admin) {
    const g = await api('PUT', '/api/config/global', {
      udp_sources: [],
      cluster_nodes: [],
      broadcast_destinations: server.cfg.passthrough ?? [],
      // Sent every save. The server treats an ABSENT field as "leave as-is"
      // and an empty string as a deliberate clear, so always sending the
      // current value keeps the Reference data box the source of truth.
      clublog_api_key: server.cfg.clublog_api_key ?? '',
    });
    if (g.status !== 200) {
      server.busy = false;
      server.error = g.json?.error ?? `HTTP ${g.status}`;
      return;
    }
  }
  server.busy = false;
  server.message = 'Applied live and saved.';
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
