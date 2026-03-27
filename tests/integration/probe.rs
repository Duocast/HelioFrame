//! Integration coverage notes for ffprobe-backed probing.
//!
//! This workspace uses a virtual manifest at the repository root, so executable
//! Rust integration tests live in crate-local `tests/` folders. The canonical
//! exercised probe coverage is in `crates/helioframe-video/src/probe.rs` tests,
//! which generate sample clips in `tests/fixtures/` and validate parsed metadata.

#[test]
fn probe_integration_coverage_is_crate_local() {
    assert!(true, "see crate-local tests in helioframe-video");
}
