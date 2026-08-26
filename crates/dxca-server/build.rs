//! Guarantees `web-ui/dist/` exists before `include_dir!` embeds it, so plain
//! `cargo build` never requires Node/pnpm (Meridian rule). When the real UI
//! has been built (`just web`), its dist is embedded instead; the stub below
//! only ever appears in a binary built on a tree that never ran the web build.

use std::fs;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dist = Path::new(&manifest).join("../../web-ui/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        fs::create_dir_all(&dist).expect("create web-ui/dist");
        fs::write(
            &index,
            "<!doctype html><meta charset=\"utf-8\"><title>DXCA</title>\
             <body style=\"font-family:system-ui;background:#0d1117;color:#c9d1d9;\
             display:grid;place-items:center;height:100vh;margin:0\">\
             <div><h1>DXCA</h1><p>Web UI not built into this binary — run \
             <code>just web</code> and rebuild.</p></div>",
        )
        .expect("write stub index.html");
    }
    println!("cargo:rerun-if-changed={}", dist.display());
}
