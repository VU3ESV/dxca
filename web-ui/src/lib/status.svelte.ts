// The server's live status, held once for the whole app.
//
// It used to be Dashboard-local, which was fine while Spots was the only
// screen that showed it. Now the header carries a health pill on every tab, so
// two things need the same object and neither may poll separately — a second
// poller would be a second answer to "is the node up", and they would disagree
// for up to five seconds at a time.
//
// Ownership: App.svelte polls slowly (the pill only needs to be roughly
// right); Dashboard pushes every `status` frame off its SSE stream straight in
// here, so while Spots is open the pill is as live as the feed.

import { api } from './api';

const s = $state<{ value: any }>({ value: null });

/** The latest status (reactive; null until the first load lands). */
export function status(): any {
  return s.value;
}

/** Push a status frame in — the stream's path into this store. */
export function setStatus(v: any): void {
  s.value = v;
}

/** Fetch it once. Safe to call repeatedly; a failure leaves the last good
 * value in place rather than blanking the pill, because a momentarily
 * unreachable server is not the same as a server with nothing to report. */
export async function refreshStatus(): Promise<void> {
  const r = await api('GET', '/api/status');
  if (r.status === 200 && r.json) s.value = r.json;
}

/** How many configured cluster nodes are proven, and how many there are —
 * the two numbers the header pill shows. */
export function nodeHealth(): { proven: number; total: number } {
  const nodes = Object.values(s.value?.cluster_nodes ?? {}) as any[];
  return { proven: nodes.filter((n) => n.proven).length, total: nodes.length };
}
