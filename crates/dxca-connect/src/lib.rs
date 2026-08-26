//! I/O engines (plan §1). Landed in M2: the WSJT-X UDP source listener,
//! the UDP broadcaster (cluster / wsjtx / passthrough formats, v1.8.3
//! counter semantics), and the built-in telnet cluster server (1.x
//! parity: banner + fan-out, no login — Meridian's login-capable server
//! comes with per-user feeds, plan §5 phase 2).
//!
//! M3 added `dxcluster`: the DX-cluster telnet **client** lifted from
//! `meridian-core/src/dxcluster/` with the v1.8.x honest-status graft
//! (password auth, proven-live tracking, fixed backoff schedule reset only
//! on proof, auth/silence watchdog, Telnet IAC stripping).
//!
//! Still to come:
//! - M4: `clublog`, `lotw`, `telegram` HTTP clients (ureq + rustls).
//!
//! Lift rules (plan §6): lifted meridian code stays diff-minimal with
//! `// DXCA:` divergence markers. This crate never imports axum, SQLite,
//! or auth.

pub mod broadcast;
pub mod dxcluster;
pub mod telnet;
pub mod wsjtx_udp;
