#!/usr/bin/env python3
"""Throwaway stand-in for dxca-server, so the rebuilt UI can be driven without
building the Rust workspace or touching the production Pi. Read-only in spirit:
every write returns ok and changes nothing.

Deliberately stocked with the WORST CASE for the new fixed-width feed table —
the longest DXCC entity names, the longest configured source name, a compound
call, 23cm frequencies — because those are the values the widths were measured
against and the whole point of running it is to see them land."""
import json, time, random
from http.server import BaseHTTPRequestHandler, HTTPServer

NOW = int(time.time())

BANDS = ["160M","80M","60M","40M","30M","20M","17M","15M","12M","10M","6M","4M","2M","1.25M","70CM"]
LEVELS = [
    {"key":"newDXCC","label":"NEW DXCC"},{"key":"newBand","label":"New Band"},
    {"key":"newMode","label":"New Mode"},{"key":"newSlot","label":"New Slot"},
    {"key":"unconfDXCC","label":"? DXCC"},{"key":"unconfBand","label":"? Band"},
    {"key":"unconfMode","label":"? Mode"},{"key":"unconfSlot","label":"? Slot"},
]
SOURCES = ["MSHV","DB0SUE","W3LPL","VU2OY","N2WQ-2","VU2CPL","UberSDR CWskim","Meridian"]
# The stress cases first: the two longest names in cty.xml, the ambiguous pair,
# and a handful of ordinary ones.
DXCC = [
    "NEW ZEALAND SUBANTARCTIC ISLANDS", "TRISTAN DA CUNHA & GOUGH ISLANDS",
    "REPUBLIC OF SOUTH AFRICA", "REPUBLIC OF SOUTH SUDAN",
    "UNITED STATES OF AMERICA", "FRANZ JOSEF LAND", "EUROPEAN RUSSIA",
    "CROATIA", "BAHRAIN", "ROMANIA", "SLOVENIA", "INDIA", "PHILIPPINES",
    "SOUTH SHETLAND ISLANDS", "DEM. REP. OF THE CONGO",
]
CALLS = ["RI1FJL","9A2WA","RM6OS","S58N","A92GE","ZS6TIG","4F2RXB","YO3GCL","VP8/GM0HCQ","VK9/W1ABC","UR5LAK","R7GR"]
MODES = ["FT8","FT4","CW","SSB","MSK144","JT65"]
ALERTS = ["none","none","none","newDXCC","newBand","newMode","newSlot","unconfDXCC","unconfBand","none","none"]

random.seed(7)

def spot(i):
    band = BANDS[i % len(BANDS)]
    khz = {"160M":1824.0,"80M":3573.0,"60M":5357.0,"40M":7074.0,"30M":10123.0,
           "20M":14074.0,"17M":18100.0,"15M":21074.0,"12M":24915.0,"10M":28074.7,
           "6M":50313.0,"4M":70154.0,"2M":144174.0,"1.25M":222100.0,"70CM":432174.0}[band]
    src = SOURCES[i % len(SOURCES)]
    skim = src in ("VU2CPL","VU2OY","UberSDR CWskim")
    return {
        "time_unix": NOW - i * 7,
        "source_name": src,
        "spotter": ("VU2CPL-9" if skim else random.choice(["RA3MU","SP3VSC","RF9C","PD2WL"])) if i % 9 else None,
        "is_skimmer": skim,
        "dx_call": CALLS[i % len(CALLS)],
        "is_lotw": i % 3 == 0,
        "dial_frequency_hz": int(khz * 1000),
        "delta_frequency_hz": 0,
        "mode": MODES[i % len(MODES)],
        "mode_inferred": i % 11 == 0,
        "snr_db": random.randint(-24, 3),
        "band": band,
        "dxcc_name": DXCC[i % len(DXCC)],
        "alert": ALERTS[i % len(ALERTS)],
        "is_beacon": i % 23 == 0,
        "is_cq": i % 2 == 0,
        "comment": "CQ LR40" if i % 4 else "FT8 -11dB from JN83 2508H JN83 long comment that must clip",
        "message": "CQ TEST",
        "band_open": (i % 5) != 0,
    }

SPOTS = [spot(i) for i in range(220)]

NODES = {n: {"state": "proven" if i % 4 else "connected", "proven": i % 4 != 0,
             "connected": True, "spot_count": 100 + i * 17,
             "last_spot_unix": NOW - i * 3, "attempt": 1}
         for i, n in enumerate(["DB0SUE","W3LPL","VU2OY","N2WQ-2","Meridian","UberCW","Hamalert","KST2Mac","VU2CPL","VU2OY-2"])}

STATUS = {
    "version": "2.12.0-dev", "milestone": "M7", "users": 3,
    "cty_entities": 402, "lotw_users": 234734, "telnet_clients": 1,
    "udp_sent": 749, "udp_failed": 0, "setup_required": False,
    "cluster_nodes": NODES,
    "spots_per_source": {s: 100 + i * 31 for i, s in enumerate(SOURCES)},
}

STATS = {
    "dxcc_worked": 320, "dxcc_confirmed": 319,
    "challenge_worked": 2438, "challenge_confirmed": 2402,
    "slots_worked": 4343, "slots_confirmed": 4082,
}
BM = {"bands": [{"key": b, "worked": 200 - i * 9, "confirmed": 195 - i * 9} for i, b in enumerate(BANDS[:12])],
      "modes": [{"key": m, "worked": 300 - i * 40, "confirmed": 290 - i * 40} for i, m in enumerate(["CW","PHONE","DATA"])]}

ROUTES = {
    "/api/status": lambda: STATUS,
    "/api/me": lambda: {"id": 1, "callsign": "VU2CPL", "display_name": "Manoj", "role": "admin"},
    "/api/reference": lambda: {"bands": BANDS, "modes": ["CW","PHONE","DATA"], "levels": LEVELS},
    "/api/spots": lambda: {"spots": SPOTS},
    "/api/me/station": lambda: {"callsign": "VU2CPL", "log_callsign": "VU2CPL", "display_name": "Manoj",
                                "qso_count": 56844, "last_refresh_unix": NOW - 3600,
                                "stats": STATS, "stats_current": STATS,
                                "by_band_mode": BM, "by_band_mode_current": BM},
    "/api/config/me/station": lambda: {"locator": "MK82", "greyline_window_min": 45},
    "/api/me/sun": lambda: {"phase": "day", "sunrise_unix": NOW - 7000, "sunset_unix": NOW + 20000,
                            "locator": "MK82", "greyline_window_min": 45},
    "/api/config/me/clublog": lambda: {"callsign": "VU2CPL", "email": "vu2cpl@example.com",
                                       "app_password": "", "refresh_hours": 24,
                                       "alert_new_dxcc": True, "alert_new_band": True,
                                       "alert_new_mode": True, "alert_new_slot": True,
                                       "alert_unconf_dxcc": False, "alert_unconf_band": False,
                                       "alert_unconf_mode": False, "alert_unconf_slot": False},
    "/api/config/me/notifications": lambda: {"telegram_enabled": True, "telegram_bot_token": "x",
                                             "telegram_chat_id": "123", "cooldown_minutes": 15,
                                             "notify_new_dxcc": True, "notify_new_band": True,
                                             "notify_new_mode": False, "notify_new_slot": True,
                                             "notify_unconf_dxcc": False, "notify_unconf_band": False,
                                             "notify_unconf_mode": False, "notify_unconf_slot": False,
                                             "notify_bands": ["20M","15M"], "notify_modes": [],
                                             "notify_manual_only": True, "notify_respect_band_mask": True},
    "/api/me/alerts": lambda: {"alerts": [
        {"time_unix": NOW - 900, "callsign": "RI1FJL", "frequency_hz": 14074000, "mode": "FT8",
         "band": "20M", "dxcc_name": "FRANZ JOSEF LAND", "level": "newDXCC", "source": "W3LPL",
         "spotter": "RA3MU", "snr_db": -11, "delivered": True, "error": None},
        {"time_unix": NOW - 4000, "callsign": "ZS6TIG", "frequency_hz": 21074000, "mode": "FT8",
         "band": "15M", "dxcc_name": "REPUBLIC OF SOUTH AFRICA", "level": "newBand", "source": "VU2OY",
         "spotter": "VU2OY", "snr_db": None, "delivered": False, "error": "Telegram 429: too many requests"}]},
    "/api/spot-stats": lambda: {
        "total": 708, "span_secs": 1920,
        "bands": [{"key": b, "count": 200 - i * 12} for i, b in enumerate(BANDS[:12])],
        "modes": [{"key": m, "count": 400 - i * 90} for i, m in enumerate(MODES)],
        "sources": [{"key": s, "count": 180 - i * 20} for i, s in enumerate(SOURCES)]},
    "/api/config/global": lambda: {
        "clublog_api_key": "secret",
        "cty_last_refresh_unix": NOW - 200000, "lotw_last_refresh_unix": NOW - 90000,
        "udp_sources": [{"name": "MSHV", "port": 2336, "enabled": True},
                        {"name": "UberSDR CWskim", "port": 2337, "enabled": True}],
        "cluster_nodes": [{"name": n, "host": f"{n.lower()}.example.net", "port": 7300,
                           "login_call": "VU2CPL", "password": "", "enabled": True}
                          for n in ["DB0SUE", "W3LPL", "VU2OY"]],
        "broadcast_destinations": [{"name": "logger", "ip": "127.0.0.1", "port": 2237,
                                    "format": "passthrough", "sources": [], "unfiltered": False,
                                    "enabled": True}],
        "read_only": {"web_bind": "0.0.0.0:80", "telnet_port": 7300, "dedupe_window_secs": 60,
                      "spot_ring_capacity": 5000, "cty_refresh_days": 7, "lotw_refresh_days": 7,
                      "data_dir": "/var/lib/dxca"}},
    "/api/mqtt": lambda: {"destinations": [{"name": "shack", "host": "192.168.1.169", "port": 1883,
                                            "username": "svc", "password": "", "topic": "shack/dxca/spots",
                                            "client_id": "dxca", "sources": [], "unfiltered": False,
                                            "enabled": True}], "sent": 512, "failed": 0, "connected": 1},
    "/api/users": lambda: {"users": [
        {"id": 1, "callsign": "VU2CPL", "display_name": "Manoj", "role": "admin"},
        {"id": 2, "callsign": "VU3ESV", "display_name": "Vinod", "role": "user"}]},
    "/api/blacklist": lambda: {"calls": ["R1ABC", "N0CALL"]},
}


class H(BaseHTTPRequestHandler):
    def _send(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?")[0]
        fn = ROUTES.get(path)
        self._send(fn() if fn else {"error": f"no mock for {path}"}, 200 if fn else 404)

    def _write(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        self._send({"ok": True, "destinations": ROUTES["/api/mqtt"]()["destinations"],
                    "connected": 1, "cty_entities": 402, "lotw_users": 234734,
                    "qso_count": 56844, "dxcc_count": 320, "calls": ["R1ABC"]})

    do_PUT = do_POST = do_PATCH = do_DELETE = _write

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 7580), H).serve_forever()
