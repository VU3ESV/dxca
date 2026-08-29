#!/usr/bin/env python3
"""Emit WSJT-X UDP packets at a local DXCA, to exercise the real pipeline
without a radio and without logging in to a cluster node.

    scripts/feed-spots.py [port]      # default 2400

Why it exists: verifying anything spot-shaped locally otherwise means
pointing a dev instance at the real cluster nodes, and `config/dxca.toml`
logs in as VU2CPL — which fights the shack Pis for the same DXSpider
session. This gives a real feed through the real decoder path with no
outside contact at all. It sends spots across four bands, chosen so that
whatever the sun is doing some are plausible and some are not, which is what
makes it useful for the phase-rotation band mask.

Set up a matching instance with its OWN directory (the config path is
hard-coded relative to the working directory, so a dev run started from the
repo root picks up the burn-in config and dials the real nodes):

    mkdir -p /tmp/dxca-dev/config && cd /tmp/dxca-dev
    cat > config/dxca.toml <<EOF
    web_bind = "127.0.0.1:7581"
    telnet_port = 7576
    data_dir = "data"
    [[udp_sources]]
    name = "WSJTX"
    port = 2400
    EOF
    /path/to/dxca

Wire format read off crates/dxca-core/src/wsjtx.rs: big-endian, magic
0xADBCCBDA, then schema and type as u32; strings are a u32 byte count then
UTF-8. A Status sets the dial frequency the following Decodes are relative
to, so bands are selected by sending a new Status.
"""
import socket
import struct
import sys
import time

MAGIC = 0xADBCCBDA
SCHEMA = 2
STATUS, DECODE = 1, 2
CLIENT = "WSJT-X"


def s(v):
    b = v.encode()
    return struct.pack(">I", len(b)) + b


def header(t):
    return struct.pack(">III", MAGIC, SCHEMA, t)


def status(dial_hz, mode):
    return (
        header(STATUS)
        + s(CLIENT)
        + struct.pack(">Q", dial_hz)
        + s(mode)
        + s("")            # dx_call
        + s("")            # report
        + s(mode)          # tx_mode
        + b"\x00\x00\x00"  # tx_enabled, transmitting, decoding
        + struct.pack(">II", 1500, 1500)
        + s("VU2CPL")
        + s("MK82")
        + s("")
    )


def decode(df_hz, snr, mode, message):
    ms = int((time.time() % 86400) * 1000)
    return (
        header(DECODE)
        + s(CLIENT)
        + b"\x01"                      # is_new
        + struct.pack(">Ii", ms, snr)
        + struct.pack(">d", 0.2)       # delta_time_s
        + struct.pack(">I", df_hz)
        + s(mode)
        + s(message)
        + b"\x00\x00"                  # low_confidence, off_air
    )


# Two bands chosen to sit on opposite sides of the mask at any hour: 160m is
# implausible in daylight, 15m is implausible at night. Whichever way the sun
# is, one group dims and the other does not — which is the thing to look at.
BANDS = [
    (1_840_000, "FT8", ["CQ VK9XX RG29", "CQ 3B8CW LH67", "CQ ZL4AS RE54"]),
    (21_074_000, "FT8", ["CQ JA1ABC PM95", "CQ VU2XYZ MK82", "CQ 9M2TO OJ03"]),
    (7_074_000, "FT8", ["CQ EA8BH IL18", "CQ VK6LC OF78"]),
    (28_074_000, "FT8", ["CQ PY2XB GG66", "CQ W1AW FN31"]),
]

port = int(sys.argv[1]) if len(sys.argv) > 1 else 2400
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sent = 0
for dial, mode, msgs in BANDS:
    sock.sendto(status(dial, mode), ("127.0.0.1", port))
    time.sleep(0.15)
    for i, m in enumerate(msgs):
        sock.sendto(decode(1200 + i * 130, -8 - i * 3, mode, m), ("127.0.0.1", port))
        sent += 1
        time.sleep(0.1)
print(f"sent {sent} decodes across {len(BANDS)} bands to 127.0.0.1:{port}")
