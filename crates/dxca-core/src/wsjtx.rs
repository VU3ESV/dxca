//! WSJT-X binary UDP codec — port of the Swift `WSJTXMessageParser` /
//! `WSJTXMessageBuilder` from DXClusterAggregator-macOS (M1, plan §3).
//!
//! Wire format is Qt QDataStream: all integers big-endian; strings are a
//! u32 byte count followed by UTF-8 bytes, with `0xFFFF_FFFF` meaning null
//! (parsed as ""). Every datagram starts magic / schema / type (u32 each).
//!
//! Parsing is deliberately **permissive**, matching the Swift parser
//! field-for-field: real emitters (MSHV, JTDX, older WSJT-X) trim or extend
//! trailing fields, and a strict parser loses whole Status messages — and
//! with them the dial frequency every Decode depends on. Only the fields
//! the Swift parser required are required here; the rest default. Invalid
//! UTF-8 in a string parses as "" (Swift `String(data:encoding:) ?? ""`),
//! and an unknown message type fails the whole parse (Swift enum-init nil).

/// `0xADBCCBDA`, first field of every WSJT-X datagram.
pub const MAGIC: u32 = 0xADBC_CBDA;
/// Schema the builder emits (WSJT-X 2.x baseline, same as the Swift builder).
pub const SCHEMA_VERSION: u32 = 2;
/// Client id the aggregator identifies as when synthesizing messages.
pub const DEFAULT_CLIENT_ID: &str = "DXClusterAggregator";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    Heartbeat = 0,
    Status = 1,
    Decode = 2,
    Clear = 3,
    Reply = 4,
    QsoLogged = 5,
    Close = 6,
    Replay = 7,
    HaltTx = 8,
    FreeText = 9,
    Wspr = 10,
    Location = 11,
    LoggedAdif = 12,
    HighlightCallsign = 13,
    SwitchConfig = 14,
    Configure = 15,
}

impl MessageType {
    fn from_raw(raw: u32) -> Option<Self> {
        use MessageType::*;
        Some(match raw {
            0 => Heartbeat,
            1 => Status,
            2 => Decode,
            3 => Clear,
            4 => Reply,
            5 => QsoLogged,
            6 => Close,
            7 => Replay,
            8 => HaltTx,
            9 => FreeText,
            10 => Wspr,
            11 => Location,
            12 => LoggedAdif,
            13 => HighlightCallsign,
            14 => SwitchConfig,
            15 => Configure,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Status {
    pub client_id: String,
    pub dial_frequency_hz: u64,
    pub mode: String,
    pub dx_call: String,
    pub report: String,
    pub tx_mode: String,
    pub tx_enabled: bool,
    pub transmitting: bool,
    pub decoding: bool,
    pub rx_df: u32,
    pub tx_df: u32,
    pub de_call: String,
    pub de_grid: String,
    pub dx_grid: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decode {
    pub client_id: String,
    pub is_new: bool,
    /// Milliseconds since midnight UTC.
    pub time_ms: u32,
    pub snr_db: i32,
    pub delta_time_s: f64,
    pub delta_frequency_hz: u32,
    pub mode: String,
    pub message: String,
    pub low_confidence: bool,
    pub off_air: bool,
}

/// A parsed datagram: the schema from the header, the message type, and —
/// for the two types the aggregator consumes — the decoded body. All other
/// types parse to `Other` (the Swift parser returned `(type, nil)`).
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Status(Status),
    Decode(Decode),
    Other(MessageType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub schema: u32,
    pub message: Message,
}

/// Parse one UDP datagram. `None` mirrors every case the Swift parser
/// rejected: bad magic, truncated header, unknown type, or a missing
/// *required* field of Status/Decode.
pub fn parse(data: &[u8]) -> Option<Parsed> {
    let mut r = Reader { data, offset: 0 };
    if r.u32()? != MAGIC {
        return None;
    }
    let schema = r.u32()?;
    let msg_type = MessageType::from_raw(r.u32()?)?;

    let message = match msg_type {
        MessageType::Status => Message::Status(parse_status(&mut r)?),
        MessageType::Decode => Message::Decode(parse_decode(&mut r)?),
        other => Message::Other(other),
    };
    Some(Parsed { schema, message })
}

/// Required: client_id + dial frequency. Everything after is best-effort
/// with defaults — see the module doc for why.
fn parse_status(r: &mut Reader) -> Option<Status> {
    let client_id = r.string()?;
    let dial_frequency_hz = r.u64()?;
    Some(Status {
        client_id,
        dial_frequency_hz,
        mode: r.string().unwrap_or_default(),
        dx_call: r.string().unwrap_or_default(),
        report: r.string().unwrap_or_default(),
        tx_mode: r.string().unwrap_or_default(),
        tx_enabled: r.bool().unwrap_or(false),
        transmitting: r.bool().unwrap_or(false),
        decoding: r.bool().unwrap_or(false),
        rx_df: r.u32().unwrap_or(0),
        tx_df: r.u32().unwrap_or(0),
        de_call: r.string().unwrap_or_default(),
        de_grid: r.string().unwrap_or_default(),
        dx_grid: r.string().unwrap_or_default(),
    })
}

/// Required: everything except the leading is_new flag (default true) and
/// the two trailing flags (default false).
fn parse_decode(r: &mut Reader) -> Option<Decode> {
    let client_id = r.string()?;
    let is_new = r.bool().unwrap_or(true);
    let time_ms = r.u32()?;
    let snr_db = r.i32()?;
    let delta_time_s = r.f64()?;
    let delta_frequency_hz = r.u32()?;
    let mode = r.string()?;
    let message = r.string()?;
    Some(Decode {
        client_id,
        is_new,
        time_ms,
        snr_db,
        delta_time_s,
        delta_frequency_hz,
        mode,
        message,
        low_confidence: r.bool().unwrap_or(false),
        off_air: r.bool().unwrap_or(false),
    })
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.offset.checked_add(n)?;
        let s = self.data.get(self.offset..end)?;
        self.offset = end;
        Some(s)
    }

    fn bool(&mut self) -> Option<bool> {
        Some(self.take(1)?[0] != 0)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_bits(self.u64()?))
    }

    /// QDataStream string. Null (0xFFFF_FFFF) and empty both parse as "";
    /// undecodable UTF-8 parses as "" too, exactly like the Swift parser —
    /// the *bytes were consumed*, keeping the cursor in sync.
    fn string(&mut self) -> Option<String> {
        let len = self.u32()?;
        if len == 0xFFFF_FFFF || len == 0 {
            return Some(String::new());
        }
        let bytes = self.take(len as usize)?;
        Some(
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .unwrap_or_default(),
        )
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// The synthesized Status carries this as the operator callsign. Using the
/// DX call there makes downstream loggers treat the pair as the operator's
/// own loop-back and suppress the spot (see the Swift builder's comment).
pub const AGGREGATOR_DE_CALL: &str = "DXCAGGR";

/// Encode a Type-1 Status with the given schema (emit `SCHEMA_VERSION`
/// unless mirroring a parsed datagram's schema for a round-trip).
pub fn encode_status(schema: u32, s: &Status) -> Vec<u8> {
    let mut w = Writer::header(schema, MessageType::Status);
    w.string(&s.client_id);
    w.u64(s.dial_frequency_hz);
    w.string(&s.mode);
    w.string(&s.dx_call);
    w.string(&s.report);
    w.string(&s.tx_mode);
    w.bool(s.tx_enabled);
    w.bool(s.transmitting);
    w.bool(s.decoding);
    w.u32(s.rx_df);
    w.u32(s.tx_df);
    w.string(&s.de_call);
    w.string(&s.de_grid);
    w.string(&s.dx_grid);
    w.out
}

/// Encode a Type-2 Decode.
pub fn encode_decode(schema: u32, d: &Decode) -> Vec<u8> {
    let mut w = Writer::header(schema, MessageType::Decode);
    w.string(&d.client_id);
    w.bool(d.is_new);
    w.u32(d.time_ms);
    w.u32(d.snr_db as u32);
    w.f64(d.delta_time_s);
    w.u32(d.delta_frequency_hz);
    w.string(&d.mode);
    w.string(&d.message);
    w.bool(d.low_confidence);
    w.bool(d.off_air);
    w.out
}

/// Status + Decode pair for one aggregated spot, ready to send
/// back-to-back — dial = exact spot frequency, delta 0 ("tuned right on top
/// of it"), same trick as the Swift builder. `time_ms` is milliseconds
/// since midnight UTC; the caller supplies it (dxca-core has no clock —
/// // DXCA: divergence from the Swift builder, which read Date() itself).
pub fn encode_spot(
    callsign_message: &str,
    frequency_hz: u64,
    snr_db: i32,
    mode: &str,
    time_ms: u32,
) -> (Vec<u8>, Vec<u8>) {
    let status = Status {
        client_id: DEFAULT_CLIENT_ID.into(),
        dial_frequency_hz: frequency_hz,
        mode: mode.into(),
        de_call: AGGREGATOR_DE_CALL.into(),
        ..Status::default()
    };
    let decode = Decode {
        client_id: DEFAULT_CLIENT_ID.into(),
        is_new: true,
        time_ms,
        snr_db,
        delta_time_s: 0.0,
        delta_frequency_hz: 0,
        mode: mode.into(),
        message: callsign_message.into(),
        low_confidence: false,
        off_air: false,
    };
    (
        encode_status(SCHEMA_VERSION, &status),
        encode_decode(SCHEMA_VERSION, &decode),
    )
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn header(schema: u32, msg_type: MessageType) -> Self {
        let mut w = Writer {
            out: Vec::with_capacity(64),
        };
        w.u32(MAGIC);
        w.u32(schema);
        w.u32(msg_type as u32);
        w
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    fn bool(&mut self, v: bool) {
        self.out.push(v as u8);
    }

    /// u32 byte count + UTF-8 bytes; empty encodes as length 0 (the Swift
    /// builder never emits the 0xFFFF_FFFF null form either).
    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.out.extend_from_slice(s.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_magic_and_unknown_type() {
        assert!(parse(&[0u8; 12]).is_none());
        let mut w = Writer::header(2, MessageType::Heartbeat);
        w.out[8..12].copy_from_slice(&99u32.to_be_bytes());
        assert!(parse(&w.out).is_none());
    }

    #[test]
    fn header_only_types_parse_as_other() {
        let w = Writer::header(2, MessageType::Heartbeat);
        let p = parse(&w.out).unwrap();
        assert_eq!(p.schema, 2);
        assert_eq!(p.message, Message::Other(MessageType::Heartbeat));
    }

    #[test]
    fn decode_roundtrips_through_encode() {
        let d = Decode {
            client_id: "JTDX".into(),
            is_new: true,
            time_ms: 41_130_000,
            snr_db: -17,
            delta_time_s: 0.2,
            delta_frequency_hz: 1487,
            mode: "~".into(),
            message: "CQ P5DX PM95".into(),
            low_confidence: false,
            off_air: false,
        };
        let bytes = encode_decode(3, &d);
        let p = parse(&bytes).unwrap();
        assert_eq!(p.schema, 3);
        assert_eq!(p.message, Message::Decode(d));
    }

    #[test]
    fn status_roundtrips_through_encode() {
        let s = Status {
            client_id: "WSJT-X".into(),
            dial_frequency_hz: 14_074_000,
            mode: "FT8".into(),
            de_call: "VU2CPL".into(),
            de_grid: "MK83".into(),
            ..Status::default()
        };
        let bytes = encode_status(2, &s);
        let p = parse(&bytes).unwrap();
        assert_eq!(p.message, Message::Status(s));
    }

    #[test]
    fn truncated_status_keeps_required_fields() {
        // A Status cut off right after the dial frequency still parses,
        // with every trailing field defaulted (the permissiveness rule).
        let s = Status {
            client_id: "MSHV".into(),
            dial_frequency_hz: 7_074_000,
            ..Status::default()
        };
        let full = encode_status(2, &s);
        let cut = &full[..12 + 4 + 4 + 8]; // header + "MSHV" string + u64
        let p = parse(cut).unwrap();
        assert_eq!(p.message, Message::Status(s));
    }

    #[test]
    fn null_string_parses_as_empty() {
        let mut w = Writer::header(2, MessageType::Status);
        w.u32(0xFFFF_FFFF); // null client id
        w.u64(14_074_000);
        let p = parse(&w.out).unwrap();
        match p.message {
            Message::Status(s) => {
                assert_eq!(s.client_id, "");
                assert_eq!(s.dial_frequency_hz, 14_074_000);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }
}
