//! DX-cluster telnet **client** — lifted from the Meridian project's
//! `meridian-core/src/dxcluster/` (plan §6), client half only (the server
//! half stays upstream; DXCA's own telnet server lives in
//! `crate::telnet` until per-user feeds need the login-capable one).
//!
//! LICENSE: this module (mod.rs, client.rs, wire.rs) is derived from
//! Meridian — © the Meridian authors (Basil Thomas W6BT, Vinod VU3ESV,
//! Ram VU3RDD), Apache-2.0 — and remains under Apache-2.0 (see
//! LICENSE-APACHE at the repo root), unlike the rest of this MIT repo.
//! DXCA's modifications are marked `// DXCA:`.
//!
//! DXCA grafts onto the lift — each marked `// DXCA:` at the site:
//!  - password prompt support (1.x clusters use username+password);
//!  - the v1.8.x **honest-status** semantics: a session is *proven live*
//!    only on real evidence (node prompt, welcome line, or actual data),
//!    never on the login timeout; the reconnect backoff follows the 1.x
//!    fixed schedule and resets only on proven-live; a watchdog recycles
//!    sessions that connect but never prove, or go silent;
//!  - Telnet IAC stripping (`wire::strip_telnet_iac`, from the 1.x client).

mod client;
pub mod wire;

pub use client::ClusterClient;
pub use wire::{LineClass, ParsedSpot};

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs()
}

/// What kind of transmission a spot reports (comment type token).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpotKind {
    Cq,
    /// The station called `CQ DX`.
    Dx,
    De,
    /// Personal beacon — the callsign carries a `/B` suffix.
    Bcn,
    /// NCDXF/IARU beacon, positively identified.
    Ncdxf,
    #[default]
    Unknown,
}

impl SpotKind {
    /// The comment's type token, or `None` for the kinds that render blank.
    pub fn token(self) -> Option<&'static str> {
        match self {
            SpotKind::Cq => Some("CQ"),
            SpotKind::Dx => Some("DX"),
            SpotKind::Bcn => Some("BCN"),
            SpotKind::Ncdxf => Some("NCDXF"),
            SpotKind::De | SpotKind::Unknown => None,
        }
    }
}

/// One spot as the client submits it upstream (`dx` command). Unused by the
/// DXCA pipeline today (1.x never submits) but kept with the lift so the
/// submission path stays available and the diff against meridian stays
/// small.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterSpot {
    /// RF frequency in kHz (wire unit).
    pub freq_khz: f64,
    pub call: String,
    pub mode: String,
    pub snr_db: i32,
    /// WPM for CW; baud for RTTY; 0 = unknown.
    pub wpm: u32,
    pub grid: Option<String>,
    pub kind: SpotKind,
}

/// Configuration for one [`ClusterClient`] (outbound connection to a node).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    pub host: String,
    pub port: u16,
    /// Callsign sent at the login prompt.
    pub login_call: String,
    // DXCA: password sent at a password prompt; empty = the node has none.
    pub password: String,
    /// Case-insensitive substrings that mark a login prompt.
    pub login_prompts: Vec<String>,
    // DXCA: case-insensitive substrings that mark a password prompt (1.x set).
    pub password_prompts: Vec<String>,
    /// Commands sent once logged in, paced one per prompt sighting.
    pub init_commands: Vec<String>,
    /// Send a bare CRLF after this many idle seconds (NAT keepalive); 0 = off.
    pub keepalive_secs: u64,
    // DXCA: 1.x fixed reconnect schedule (last entry repeats) instead of
    // meridian's doubling min/max — and the supervisor resets the attempt
    // index only on proven-live, never on bare TCP connect.
    pub reconnect_schedule_s: Vec<u64>,
    // DXCA: watchdog — recycle when connected this long without proven-live.
    pub auth_timeout_s: u64,
    // DXCA: watchdog — recycle a proven session after this much rx silence.
    pub silence_timeout_s: u64,
}

impl ClientConfig {
    /// Config with house defaults on everything but the endpoint + call.
    pub fn new(host: &str, port: u16, login_call: &str) -> Self {
        ClientConfig {
            host: host.to_string(),
            port,
            login_call: login_call.to_string(),
            password: String::new(),
            login_prompts: ["login:", "callsign:", "call:"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            password_prompts: ["password:", "passwd:", "password please:"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            init_commands: vec!["set/page 0".to_string(), "set/dxgrid on".to_string()],
            keepalive_secs: 300,
            reconnect_schedule_s: vec![10, 30, 60, 120, 300],
            auth_timeout_s: 120,
            silence_timeout_s: 15 * 60,
        }
    }
}

/// What a [`ClusterClient`] reports back to its consumer.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// TCP established (login not yet done).
    Connected,
    /// Login handshake complete (prompt seen, data flowing, or timeout
    /// fallback) — the session now sends init commands / submissions.
    LoggedIn,
    // DXCA: real evidence the session works — node prompt, welcome line, or
    // actual data. Emitted at most once per connection; this (not LoggedIn,
    // which the 30 s timeout can fire) is what resets the backoff and turns
    // the status pill green.
    Proven,
    /// A `DX de` line, parsed.
    Spot {
        spot: ParsedSpot,
        raw: String,
    },
    /// A WWV/WCY solar line (raw).
    Wwv(String),
    /// A `To ALL de …` announcement (raw).
    Announce(String),
    /// Any other non-empty line (raw passthrough).
    Line(String),
    /// Connection lost; the supervisor is backing off toward a reconnect.
    Disconnected {
        reason: String,
    },
}
