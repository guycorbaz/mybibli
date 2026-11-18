//! Repository files read by configuration tests (`toolchain_pins`,
//! `tailwind_sources`).

#![allow(dead_code)] // each test crate uses a different subset

use std::fs;
use std::path::Path;

/// The text of a repository-relative file; panics with its path otherwise.
pub fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
