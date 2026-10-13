//! The compiler's own version stamp for the debug overlay.
//!
//! The overlay's version line reports the toolchain fact where the source
//! reports its LWJGL fact. Cargo runs this script with `RUSTC` pointing at
//! the compiler in use, so `$RUSTC --version` always names the toolchain
//! that builds the crate — no network, no new dependency. When the query
//! fails the variable stays unset and the overlay reads `n/a` for the fact,
//! recorded, never invented.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    if let Ok(output) = Command::new(rustc).arg("--version").output() {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout)
                .trim()
                .strip_prefix("rustc ")
                .unwrap_or("")
                .to_string();
            if !version.is_empty() {
                println!("cargo:rustc-env=OXIDECRAFT_RUSTC_VERSION={version}");
            }
        }
    }
}
