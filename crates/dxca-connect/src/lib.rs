//! I/O engines (plan §1) — empty at M0, filled per milestone:
//!
//! - M2: `wsjtx_udp` (UDP listener on 2237), `broadcast` (UDP destinations
//!   incl. passthrough with v1.8.3 fail-counter semantics), plus the
//!   DX-cluster telnet **server** lifted from
//!   `meridian-core/src/dxcluster/` (server.rs + wire.rs).
//! - M3: the lifted DX-cluster telnet **client** with the v1.8.x
//!   honest-status graft.
//! - M4: `clublog`, `lotw`, `telegram` HTTP clients (ureq + rustls).
//!
//! Lift rules (plan §6): the `dxcluster/` module stays diff-minimal against
//! meridian-core; every intentional divergence carries a `// DXCA:` comment.
//! This crate never imports axum, SQLite, or auth.
