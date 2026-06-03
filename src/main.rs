//! The `hdx` binary — thin glue only.
//!
//! This bin is a JSON-emitting, LLM-drivable wrapper over the `hdx-core` verbs
//! (spec §10; architecture §2). It does exactly four things per subcommand:
//! parse one dataset path, call the corresponding `hdx-core` verb, serialize the
//! verb's returned value as JSON, and print that JSON to **stdout**. It holds
//! **no contract logic**: no §14 rule, no manifest parsing, no reader, no
//! discovery lives here — all of that is in `hdx-core`. The wire shape is the
//! `hdx-core` serializer output, reused verbatim and never re-derived.
//!
//! Output vs. diagnostics: the JSON on **stdout** is *output*. All diagnostics
//! go through `tracing` to **stderr**; `println!` is used only to emit the JSON
//! output value, never for diagnostics.
//!
//! ## The exit-code contract (spec §0/§10/§14, architecture §2)
//!
//! The process exit code is derived **solely** from the verb's `Result` — the
//! bin adds no contract logic, only result→code routing:
//!
//! | Code | Meaning |
//! |---|---|
//! | `0` | success — `describe` succeeded, **or** `validate` returned `conformant: true` |
//! | `1` | non-conformant — `validate` returned a report with `conformant: false` |
//! | `2` | usage / IO error — bad args, unreadable / nonexistent path, **malformed** manifest, or the §0 hard cut (unknown `format_version`) |
//!
//! The load-bearing distinction (spec §0 vs §14): a `conformant: false` **report**
//! (a violated `MUST` that ran) is exit **1**, *distinct* from a structural / entry
//! **error** (exit **2**). The §0 hard cut surfaces from the verb as
//! `Err(_::Manifest(CoreError::UnknownFormatVersion { .. }))` and is **never**
//! special-cased — it falls into the exit-2 `Err` arm, never softened into a report.

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing::error;

use hdx_core::describe::describe_json;
use hdx_core::validate::validate;

/// The `hdx` CLI: a thin JSON-emitting surface over the `hdx-core` verbs.
#[derive(Debug, Parser)]
#[command(name = "hdx", version, about = "Thin JSON-emitting CLI over the hdx-core verbs")]
struct Cli {
    /// The verb to run.
    #[command(subcommand)]
    command: Command,
}

/// The supported subcommands. Each wraps one `hdx-core` verb and takes a single
/// dataset path.
#[derive(Debug, Subcommand)]
enum Command {
    /// Describe a dataset: print the `describe` JSON to stdout.
    Describe {
        /// Path to the dataset root (the directory holding `manifest.json`).
        path: PathBuf,
    },
    /// Validate a dataset: print the `ValidationReport` JSON to stdout.
    Validate {
        /// Path to the dataset root (the directory holding `manifest.json`).
        path: PathBuf,
    },
}

/// The exit code for a structural / entry error (spec §0/§10): bad args,
/// unreadable / nonexistent path, malformed manifest, or the §0 hard cut.
const EXIT_ERROR: u8 = 2;

/// The exit code for a non-conformant `validate` verdict (spec §14): a report
/// with `conformant: false` (a violated `MUST` that ran).
const EXIT_NON_CONFORMANT: u8 = 1;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // A bad / missing subcommand or argument exits here with `clap`'s default
    // usage-error code (2), satisfying the exit-2 usage-error leg of the contract.
    let cli = Cli::parse();

    match cli.command {
        Command::Describe { path } => describe_exit(&path),
        Command::Validate { path } => validate_exit(&path),
    }
}

/// Runs `describe` and maps its `Result` to an exit code (no contract logic — only
/// result→code routing, spec §0/§10).
///
/// `Ok(json)` → print the JSON to **stdout**, exit **0**. `Err(_)` (unreadable /
/// nonexistent path, malformed manifest, the §0 hard cut, or a discovery fault) →
/// log to **stderr** via `tracing`, exit **2**. The hard cut
/// (`DescribeError::Manifest(UnknownFormatVersion)`) is **not** special-cased: it is
/// one of the `Err` variants and falls into the exit-2 arm.
fn describe_exit(path: &Path) -> ExitCode {
    match describe_json(path) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            error!(path = %path.display(), error = %err, "describe failed");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

/// Runs `validate` and maps its `Result` to an exit code (no contract logic — only
/// result→code routing, spec §0/§10/§14).
///
/// `Ok(report)` → print `report.to_json_string()` to **stdout**, then exit **0** if
/// `report.conformant()` else **1** (a `conformant: false` report — a violated
/// `MUST` that ran — is the load-bearing exit-1 ≠ exit-2 case). `Err(_)`
/// (unreadable / nonexistent path, malformed manifest, the §0 hard cut, or a
/// discovery fault) → log to **stderr** via `tracing`, exit **2**. The hard cut
/// (`ValidateError::Manifest(UnknownFormatVersion)`) is **not** special-cased — the
/// CLI never softens it into a `conformant: false` report.
fn validate_exit(path: &Path) -> ExitCode {
    match validate(path) {
        Ok(report) => match report.to_json_string() {
            Ok(json) => {
                println!("{json}");
                if report.conformant() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(EXIT_NON_CONFORMANT)
                }
            }
            Err(err) => {
                error!(path = %path.display(), error = %err, "serializing validation report failed");
                ExitCode::from(EXIT_ERROR)
            }
        },
        Err(err) => {
            error!(path = %path.display(), error = %err, "validate failed");
            ExitCode::from(EXIT_ERROR)
        }
    }
}
