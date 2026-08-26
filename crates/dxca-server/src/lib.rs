//! Library surface of the server crate — exists so integration tests can
//! drive the pipeline; the `dxca` binary (main.rs) is a thin consumer.

pub mod assets;
pub mod config;
pub mod nodes;
pub mod pipeline;
