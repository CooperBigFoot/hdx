//! The `hdx` binary — thin glue only.
//!
//! This bin is a JSON-emitting, LLM-drivable wrapper over the `hdx-core` verbs
//! (spec §10; architecture §2). It does exactly four things per subcommand:
//! parse one dataset path, call the corresponding `hdx-core` verb, serialize the
//! verb's returned value as JSON, and print that JSON to **stdout**. It holds
//! **no contract logic**: no §14 rule, no manifest parsing, no reader, no
//! discovery lives here — all of that is in `hdx-core`. The wire shape is the
//! MS5/MS6 serializer output, reused verbatim and never re-derived.
//!
//! Output vs. diagnostics: the JSON on **stdout** is *output*. All diagnostics
//! go through `tracing` to **stderr**; `println!` is used only to emit the JSON
//! output value, never for diagnostics.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
    /// Describe a dataset: print the `describe` JSON (MS5 shape) to stdout.
    Describe {
        /// Path to the dataset root (the directory holding `manifest.json`).
        path: PathBuf,
    },
    /// Validate a dataset: print the `ValidationReport` JSON (MS6 shape) to stdout.
    Validate {
        /// Path to the dataset root (the directory holding `manifest.json`).
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Describe { path } => {
            let json = describe_json(&path)
                .with_context(|| format!("describe failed for {}", path.display()))?;
            println!("{json}");
        }
        Command::Validate { path } => {
            let report = validate(&path)
                .with_context(|| format!("validate failed for {}", path.display()))?;
            let json = report
                .to_json_string()
                .with_context(|| format!("serializing validation report for {}", path.display()))?;
            println!("{json}");
        }
    }

    Ok(())
}
