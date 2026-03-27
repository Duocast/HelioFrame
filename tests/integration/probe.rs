//! Integration coverage is implemented in crate-local tests at
//! `crates/helioframe-video/tests/io_regression.rs`.
//!
//! This workspace root uses a virtual manifest, so runnable Rust integration
//! tests must live inside crate directories.

#[test]
fn integration_probe_regression_is_crate_local() {
    assert!(true);
}
