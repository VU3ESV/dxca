#!/usr/bin/env python3
"""Extract WSJT-X binary test vectors from a loopback pcap of the shack
UDP pipeline (see DXClusterAggregator-macOS docs/UDP-PIPELINE.md).

Reads a tcpdump capture taken on macOS lo0 (DLT_NULL) and:
  - attributes each UDP datagram to its decoder by destination port
    (2333 MSHV, 2334 JTDX, 2335 WSJT-X; 2237 is DXCA's passthrough out,
    2233 is raw ADIF text);
  - classifies WSJT-X messages by magic/schema/type;
  - writes the first N samples of every (decoder, type) pair to
    <outdir>/<decoder>/typeNN-<i>.bin  — these become golden-test inputs
    for the dxca-core codec (plan §10 M1);
  - writes ADIF datagrams (if any QSO was logged mid-capture) to
    <outdir>/adif/;
  - verifies every passthrough datagram is byte-identical to a previously
    seen decoder datagram (the v1.8.2 passthrough invariant, useful for M2);
  - dumps a summary.json with per-decoder/per-type counts.

Usage: extract_vectors.py <capture.pcap> <outdir> [--per-type N]
"""

import json
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

PORT_NAMES = {2333: "mshv", 2334: "jtdx", 2335: "wsjtx", 2237: "passthrough", 2233: "adif"}
WSJTX_MAGIC = 0xADBCCBDA

# WSJT-X NetworkMessage.hpp message types, for the summary's human labels.
TYPE_NAMES = {
    0: "Heartbeat", 1: "Status", 2: "Decode", 3: "Clear", 4: "Reply",
    5: "QSOLogged", 6: "Close", 7: "Replay", 8: "HaltTx", 9: "FreeText",
    10: "WSPRDecode", 11: "Location", 12: "LoggedADIF", 13: "HighlightCallsign",
    14: "SwitchConfiguration", 15: "Configure",
}


def pcap_udp_payloads(path):
    """Yield (dst_port, payload) for UDP/IPv4 packets in a pcap file.

    Handles classic pcap (both endiannesses, micro/nanosecond) with
    linktype NULL (4-byte family header, macOS loopback) or EN10MB.
    """
    data = Path(path).read_bytes()
    magic = data[:4]
    if magic in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"):
        endian = "<"
    elif magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"):
        endian = ">"
    else:
        sys.exit(f"not a classic pcap file: {path}")
    linktype = struct.unpack(endian + "I", data[20:24])[0]
    ll_len = {0: 4, 1: 14}.get(linktype)
    if ll_len is None:
        sys.exit(f"unsupported linktype {linktype}")

    off = 24
    while off + 16 <= len(data):
        incl = struct.unpack(endian + "I", data[off + 8 : off + 12])[0]
        pkt = data[off + 16 : off + 16 + incl]
        off += 16 + incl
        ip = pkt[ll_len:]
        if len(ip) < 20 or ip[0] >> 4 != 4 or ip[9] != 17:  # IPv4, UDP
            continue
        ihl = (ip[0] & 0xF) * 4
        udp = ip[ihl:]
        if len(udp) < 8:
            continue
        dst_port = struct.unpack(">H", udp[2:4])[0]
        udp_len = struct.unpack(">H", udp[4:6])[0]
        yield dst_port, udp[8:udp_len]


def classify(payload):
    """Return (schema, msg_type) for a WSJT-X datagram, or None."""
    if len(payload) < 12:
        return None
    magic, schema, msg_type = struct.unpack(">III", payload[:12])
    if magic != WSJTX_MAGIC:
        return None
    return schema, msg_type


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    per_type = 8
    if "--per-type" in sys.argv:
        per_type = int(sys.argv[sys.argv.index("--per-type") + 1])
    if len(args) != 2:
        sys.exit(__doc__)
    pcap, outdir = args[0], Path(args[1])

    counts = defaultdict(Counter)  # decoder -> type-label -> n
    saved = Counter()  # (decoder, type) -> n saved
    schemas = defaultdict(set)
    seen_payloads = set()  # from decoder ports, for the passthrough check
    pt_total = pt_matched = 0
    bad = Counter()

    for dst_port, payload in pcap_udp_payloads(pcap):
        name = PORT_NAMES.get(dst_port)
        if name is None:
            continue
        if name == "adif":
            counts[name]["adif-text"] += 1
            i = counts[name]["adif-text"]
            d = outdir / "adif"
            d.mkdir(parents=True, exist_ok=True)
            (d / f"qso-{i}.bin").write_bytes(payload)
            continue
        if name == "passthrough":
            pt_total += 1
            pt_matched += payload in seen_payloads
            continue

        seen_payloads.add(payload)
        cls = classify(payload)
        if cls is None:
            bad[name] += 1
            continue
        schema, msg_type = cls
        schemas[name].add(schema)
        label = f"type{msg_type:02d}-{TYPE_NAMES.get(msg_type, 'Unknown')}"
        counts[name][label] += 1
        if saved[(name, msg_type)] < per_type:
            saved[(name, msg_type)] += 1
            d = outdir / name
            d.mkdir(parents=True, exist_ok=True)
            (d / f"type{msg_type:02d}-{saved[(name, msg_type)]}.bin").write_bytes(payload)

    summary = {
        "source_pcap": str(pcap),
        "per_decoder": {k: dict(sorted(v.items())) for k, v in sorted(counts.items())},
        "schemas": {k: sorted(v) for k, v in sorted(schemas.items())},
        "non_wsjtx_datagrams": dict(bad),
        "passthrough": {"datagrams": pt_total, "byte_identical_to_a_source": pt_matched},
        "samples_per_type": per_type,
    }
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
