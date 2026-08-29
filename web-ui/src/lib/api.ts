// Thin fetch/WS helpers. Session auth rides on the HttpOnly cookie —
// same-origin requests carry it automatically.

export interface ApiResult {
  status: number;
  json: any;
}

/// Never throws. A caller gets `status: 0` when the request could not be made
/// at all, and every caller in the app already branches on `status === 200`,
/// so an unreachable server degrades into the same path as a rejected one.
///
/// This used to let `fetch`'s own rejection escape, and the difference is not
/// academic: an HTTP error is a *reply*, while a route disappearing under a
/// live page is an *exception*, and only the first was being handled. On
/// 2026-08-29 a VPN came up and took the route to DXCA with it; every screen's
/// `onMount` rejected half-way through, and the Settings pages — which render
/// nothing until their config arrives — went blank with no explanation. From
/// the other side of the screen that reads as "all my settings are gone".
export const NETWORK_DOWN = 0;

export async function api(
  method: string,
  path: string,
  body?: unknown,
): Promise<ApiResult> {
  let resp: Response;
  try {
    resp = await fetch(path, {
      method,
      headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  } catch (e) {
    // DNS, refused, aborted, or the route pulled out from under us. The
    // browser deliberately gives no detail here, so say what is useful rather
    // than echoing "Load failed".
    return {
      status: NETWORK_DOWN,
      json: { error: `Cannot reach the server (${method} ${path})` },
    };
  }
  let json: any = null;
  try {
    json = await resp.json();
  } catch {
    /* non-JSON body */
  }
  return { status: resp.status, json };
}

/// Open /api/stream with auto-reconnect. Returns a close function.
export function openStream(onFrame: (frame: any) => void): () => void {
  let socket: WebSocket | null = null;
  let closed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    socket = new WebSocket(`${proto}://${location.host}/api/stream`);
    socket.onmessage = (ev) => {
      try {
        onFrame(JSON.parse(ev.data));
      } catch {
        /* ignore malformed frame */
      }
    };
    socket.onclose = () => {
      if (!closed) timer = setTimeout(connect, 3000);
    };
  }
  connect();

  return () => {
    closed = true;
    if (timer) clearTimeout(timer);
    socket?.close();
  };
}

export function hhmm(timeUnix: number): string {
  const secs = ((timeUnix % 86400) + 86400) % 86400;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${String(h).padStart(2, '0')}${String(m).padStart(2, '0')}`;
}

export function ago(unix: number | null): string {
  if (!unix) return '—';
  const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  // Days matter now that this also ages a weekly LoTW download and a log
  // refresh — "168h ago" is a number you have to do arithmetic on.
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}
