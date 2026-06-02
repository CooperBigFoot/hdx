//! `hdx` — a thin PyO3 mirror of the `hdx-core` contract verbs (spec §10).
//!
//! This crate adds **zero** contract logic: it exists only to expose `hdx-core`'s
//! `validate`/`describe` verbs to Python (the verbs themselves land in MS9-S2).
//! As of MS9-S1 it carries a single trivial function, [`__core_version`], whose
//! only job is to prove the abi3 extension links against `hdx-core` and imports
//! from Python. The PyO3 `extension-module` feature is **optional and
//! non-default** (see `Cargo.toml`): `cargo build/test/clippy` link with it OFF so
//! the `rlib` unit-test target builds on macOS; `maturin` enables it for the
//! shipped wheel only.

use pyo3::prelude::*;

/// Return this binding crate's version (the compile-time `CARGO_PKG_VERSION`).
///
/// A do-nothing-useful import/link proof: it touches no IO and no contract logic,
/// so a successful `import hdx; hdx.__core_version()` confirms the abi3 extension
/// links against `hdx-core` and imports under the host interpreter.
#[pyfunction]
fn __core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The `hdx` Python module entry point.
///
/// Registers the single MS9-S1 function. The two mirrored verbs (`validate`,
/// `describe`) are added to this module in MS9-S2.
#[pymodule]
fn hdx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(__core_version, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::__core_version;

    /// Asserts the import/link-proof function returns a non-empty version string.
    ///
    /// This test builds against the `rlib` target (HIGH-2) and runs WITHOUT the
    /// `extension-module` feature (HIGH-1) — the Rust-level macOS-link proof.
    #[test]
    fn core_version_is_non_empty() {
        assert!(!__core_version().is_empty());
    }
}
