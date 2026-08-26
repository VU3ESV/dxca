//! Pure aggregation logic — no I/O, no async, no database.
//!
//! This crate is the portable heart of DXCA (plan §1/§6): the spot model,
//! the WSJT-X binary UDP codec, the CTY/ADIF parsers, the per-user worked
//! matrix, and the New DXCC/Slot/Band/Mode classifier. It must never grow a
//! dependency on axum, SQLite, or the auth layer — that separation is what
//! keeps the later Meridian-integration door open.
//!
//! Ported piecewise from the Swift implementation in
//! DXClusterAggregator-macOS, tested against datagrams captured from the
//! live shack decoders (`tests/vectors/`).

mod spot;
pub mod wsjtx;

pub use spot::{Spot, SpotSource};
