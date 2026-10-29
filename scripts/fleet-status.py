#!/usr/bin/env python3
"""One line per DXCA host: version, nodes Live, users, telnet clients, update.

    scripts/fleet-status.py HOST [HOST ...]

Reads each host's /api/status (no login needed) and prints the facts a deploy
round checks before and after — the version it serves, how many cluster
nodes are Live, and whether its update check is showing a newer release.
Written during the v2.23.0/v2.23.1 deploys (2026-10-09/10); the per-host
drill is in HANDOVER.md under those sessions. The Windows box blocks ICMP but
answers here, so this is the reachability test for it, not ping.
"""
import json
import sys
import urllib.request


def one(host: str) -> None:
    try:
        d = json.load(urllib.request.urlopen(f"http://{host}:7580/api/status", timeout=10))
    except Exception as e:  # noqa: BLE001 — a dead host is a line, not a crash
        print(f"{host:<16} status FAILED: {e}")
        return
    n = d.get("cluster_nodes", [])
    items = n if isinstance(n, list) else list(n.values())
    live = sum(1 for x in items if isinstance(x, dict) and str(x.get("state", "")).lower() == "live")
    u = d.get("update")
    upd = f"{u.get('tag')} available" if isinstance(u, dict) else "current"
    print(
        f"{host:<16} v{d.get('version'):<8} nodes {live}/{len(items)}  users {d.get('users')}  "
        f"telnet {d.get('telnet_clients')}  cty {d.get('cty_entities')}  fcc {d.get('fcc_calls')}  {upd}"
    )


if __name__ == "__main__":
    hosts = sys.argv[1:]
    if not hosts:
        print(__doc__.strip().splitlines()[2])
        sys.exit(2)
    for h in hosts:
        one(h)
