//! `hdx` — a thin PyO3 mirror of the `hdx-core` contract verbs (spec §10).
//!
//! This crate adds **zero** contract logic: it exists only to expose `hdx-core`'s
//! `validate` / `describe` verbs to Python. Each Python function calls the matching
//! `hdx-core` `*_json` verb, parses the **already-produced** JSON string into a
//! Python `dict` (the string is reused verbatim, so the dict matches the committed
//! `schemas/describe.schema.json` / `schemas/validate.schema.json` by construction —
//! no wire shape is re-derived here), and maps the typed boundary errors to Python
//! exceptions. No §14 rule, no manifest parse, no reader, no discovery lives in this
//! crate; all contract logic is in `hdx-core`.
//!
//! ## The §0 hard cut is preserved through the binding
//!
//! A wrong `format_version` surfaces from `hdx-core` as
//! `…::Manifest(CoreError::UnknownFormatVersion { .. })` — an `Err`, never a softened
//! `conformant: false` report. The binding maps **exactly that variant** to a
//! dedicated [`UnknownFormatVersionError`] Python exception; every other boundary
//! error maps to the [`HdxError`] base. `validate` only ever returns a `dict` on
//! `Ok`; a `conformant: false` is a normal `Ok` report `dict`, distinct from any
//! raised exception (spec §0 / §10 / §14).
//!
//! The PyO3 `extension-module` feature is **optional and non-default** (see
//! `Cargo.toml`): `cargo build/test/clippy` link with it OFF so the `rlib`
//! unit-test target builds on macOS; `maturin` enables it for the shipped wheel only.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyModule;

use hdx_core::describe::describe_json;
use hdx_core::error::{CoreError, DescribeError, ValidateError};
use hdx_core::validate::validate_json;

create_exception!(
    hdx,
    HdxError,
    PyException,
    "Base for every error raised by the hdx binding (a structural / entry failure from hdx-core)."
);

create_exception!(
    hdx,
    UnknownFormatVersionError,
    HdxError,
    "The §0 hard cut: manifest.format_version is not the single supported version. Never softened into a conformant:false report."
);

/// Which Python exception type a boundary error maps to.
///
/// The §0 hard version cut is the **one** load-bearing distinction: it MUST select
/// [`HdxExceptionKind::UnknownFormatVersion`] (→ [`UnknownFormatVersionError`]) and
/// never be softened into a report. Every other structural / entry failure is
/// [`HdxExceptionKind::General`] (→ [`HdxError`]). Keeping the choice in a pure,
/// `Python`-free enum lets the mapping be unit-tested at the Rust level (the §0
/// proof) without a Python interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdxExceptionKind {
    /// The §0 hard cut — a wrong `format_version`. Maps to [`UnknownFormatVersionError`].
    UnknownFormatVersion,
    /// Any other structural / entry failure. Maps to [`HdxError`].
    General,
}

impl HdxExceptionKind {
    /// Classifies a [`ValidateError`] into its target exception kind.
    ///
    /// Only `Manifest(CoreError::UnknownFormatVersion { .. })` — the §0 hard cut —
    /// selects [`HdxExceptionKind::UnknownFormatVersion`]; every other variant
    /// (including any other `Manifest(_)`) is [`HdxExceptionKind::General`].
    fn from_validate_error(err: &ValidateError) -> Self {
        match err {
            ValidateError::Manifest(CoreError::UnknownFormatVersion { .. }) => {
                Self::UnknownFormatVersion
            }
            _ => Self::General,
        }
    }

    /// Classifies a [`DescribeError`] into its target exception kind.
    ///
    /// Only `Manifest(CoreError::UnknownFormatVersion { .. })` — the §0 hard cut —
    /// selects [`HdxExceptionKind::UnknownFormatVersion`]; every other variant is
    /// [`HdxExceptionKind::General`].
    fn from_describe_error(err: &DescribeError) -> Self {
        match err {
            DescribeError::Manifest(CoreError::UnknownFormatVersion { .. }) => {
                Self::UnknownFormatVersion
            }
            _ => Self::General,
        }
    }

    /// Builds the concrete [`PyErr`] for this kind, carrying `message` (the boundary
    /// error's `Display` text) as the exception payload.
    fn into_pyerr(self, message: String) -> PyErr {
        match self {
            Self::UnknownFormatVersion => UnknownFormatVersionError::new_err(message),
            Self::General => HdxError::new_err(message),
        }
    }
}

/// Parses an `hdx-core` `*_json` string into a Python object (a `dict`).
///
/// Transports the already-produced JSON **unchanged** through Python's stdlib
/// `json.loads`, so the returned `dict` has exactly the schema keys the verb's JSON
/// string carries (`manifest/basins/fields/grids/time_extents/delineations` for
/// describe; `checks/conformant` for validate). No wire shape is re-derived. A parse
/// failure (never expected for a verb's own valid output) surfaces as [`HdxError`].
fn json_string_to_pyobject(py: Python<'_>, json: &str) -> PyResult<PyObject> {
    let json_module = PyModule::import(py, "json")?;
    let parsed = json_module
        .call_method1("loads", (json,))
        .map_err(|err| HdxError::new_err(format!("failed to parse hdx-core JSON output: {err}")))?;
    Ok(parsed.unbind())
}

/// Validate a dataset and return the conformance report as a Python `dict`.
///
/// Mirrors [`hdx_core::validate::validate_json`] verbatim: on `Ok` it parses the
/// report JSON into a `dict` (keys `checks`, `conformant`); on `Err` it raises an
/// [`HdxError`]-family exception. A violated `MUST` that ran is a normal
/// `conformant: false` report `dict` (an `Ok`), never an exception. The §0 hard cut
/// (a wrong `format_version`) raises [`UnknownFormatVersionError`].
#[pyfunction]
fn validate(py: Python<'_>, path: &str) -> PyResult<PyObject> {
    match validate_json(path) {
        Ok(json) => json_string_to_pyobject(py, &json),
        Err(err) => Err(HdxExceptionKind::from_validate_error(&err).into_pyerr(err.to_string())),
    }
}

/// Describe a dataset and return the self-description as a Python `dict`.
///
/// Mirrors [`hdx_core::describe::describe_json`] verbatim: on `Ok` it parses the
/// description JSON into a `dict` (keys `manifest`, `basins`, `fields`, `grids`,
/// `time_extents`, `delineations`); on `Err` it raises an [`HdxError`]-family
/// exception. The §0 hard cut (a wrong `format_version`) raises
/// [`UnknownFormatVersionError`].
#[pyfunction]
fn describe(py: Python<'_>, path: &str) -> PyResult<PyObject> {
    match describe_json(path) {
        Ok(json) => json_string_to_pyobject(py, &json),
        Err(err) => Err(HdxExceptionKind::from_describe_error(&err).into_pyerr(err.to_string())),
    }
}

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
/// Registers the two mirrored verbs (`validate`, `describe`), the import/link-proof
/// `__core_version`, and the [`HdxError`] / [`UnknownFormatVersionError`] exception
/// types so callers can `except hdx.UnknownFormatVersionError` on the §0 hard cut.
#[pymodule]
fn hdx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("HdxError", m.py().get_type::<HdxError>())?;
    m.add(
        "UnknownFormatVersionError",
        m.py().get_type::<UnknownFormatVersionError>(),
    )?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(describe, m)?)?;
    m.add_function(wrap_pyfunction!(__core_version, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use hdx_core::error::{CoreError, DescribeError, ValidateError};

    use super::{HdxExceptionKind, __core_version};

    /// Asserts the import/link-proof function returns a non-empty version string.
    ///
    /// This test builds against the `rlib` target (HIGH-2) and runs WITHOUT the
    /// `extension-module` feature (HIGH-1) — the Rust-level macOS-link proof.
    #[test]
    fn core_version_is_non_empty() {
        assert!(!__core_version().is_empty());
    }

    /// §0 mapping proof (validate): the hard version cut selects the dedicated
    /// hard-cut exception kind — it is **never** softened into a report value.
    ///
    /// This asserts the load-bearing exception **type** selection without a Python
    /// interpreter (buildable only because the crate is also an `rlib`).
    #[test]
    fn validate_unknown_format_version_maps_to_hard_cut_kind() {
        let err = ValidateError::Manifest(CoreError::UnknownFormatVersion {
            found: "0.2".to_string(),
        });
        assert_eq!(
            HdxExceptionKind::from_validate_error(&err),
            HdxExceptionKind::UnknownFormatVersion,
        );
    }

    /// §0 mapping proof (describe): the hard version cut selects the dedicated
    /// hard-cut exception kind on the describe path too.
    #[test]
    fn describe_unknown_format_version_maps_to_hard_cut_kind() {
        let err = DescribeError::Manifest(CoreError::UnknownFormatVersion {
            found: "0.2".to_string(),
        });
        assert_eq!(
            HdxExceptionKind::from_describe_error(&err),
            HdxExceptionKind::UnknownFormatVersion,
        );
    }

    /// A `DescribeError::ManifestUnreadable` maps to the general `HdxError` kind
    /// (not the hard-cut exception).
    #[test]
    fn describe_manifest_unreadable_maps_to_general() {
        let err = DescribeError::ManifestUnreadable {
            path: "/no/such/dataset/manifest.json".to_string(),
            detail: "No such file or directory".to_string(),
        };
        assert_eq!(
            HdxExceptionKind::from_describe_error(&err),
            HdxExceptionKind::General,
        );
    }

    /// A generic `Discovery(_)` fault maps to the general `HdxError` kind.
    #[test]
    fn validate_discovery_maps_to_general() {
        let err = ValidateError::Discovery(CoreError::LayoutWalk {
            path: "/no/such/dir".to_string(),
            detail: "No such file or directory".to_string(),
        });
        assert_eq!(
            HdxExceptionKind::from_validate_error(&err),
            HdxExceptionKind::General,
        );
    }

    /// A non-hard-cut `Manifest(_)` boundary failure (e.g. a missing floor field)
    /// maps to the general `HdxError` kind — only `UnknownFormatVersion` is the
    /// dedicated hard-cut exception.
    #[test]
    fn validate_other_manifest_error_maps_to_general() {
        let err = ValidateError::Manifest(CoreError::MissingManifestField {
            field: "cadence".to_string(),
        });
        assert_eq!(
            HdxExceptionKind::from_validate_error(&err),
            HdxExceptionKind::General,
        );
    }
}
