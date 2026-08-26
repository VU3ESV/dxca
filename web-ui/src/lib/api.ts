// Thin fetch/WS helpers. Session auth rides on the HttpOnly cookie —
// same-origin requests carry it automatically.

export interface ApiResult {
  status: number;
  json: any;
}

export async function api(
  method: string,
  path: string,
  body?: unknown,
): Promise<ApiResult> {
  const resp = await fetch(path, {
    method,
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
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
  return `${Math.floor(s / 3600)}h`;
}
