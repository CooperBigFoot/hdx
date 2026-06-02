# `hdx` — the CLI

`hdx` is a thin, JSON-emitting, LLM-drivable command-line surface over the two HDX
v0.1 verbs (spec §10). It is **glue only**: it parses one dataset path, calls the
corresponding [`hdx-core`](crates/core/README.md) verb, serializes the verb's returned
value as JSON to **stdout**, and maps the verb's `Result` to a process exit code. It
holds **no contract logic** — every §14 rule, the manifest parser, the readers, and the
discovery layer live in `hdx-core`. See `architecture.md` §2.

## Subcommands

| Command | Verb | Stdout |
|---|---|---|
| `hdx describe <path>` | [`hdx_core::describe::describe_json`](crates/core/src/describe.rs) | the `Description` JSON (the MS5 wire shape; `schemas/describe.schema.json`) |
| `hdx validate <path>` | [`hdx_core::validate::validate`](crates/core/src/validate.rs) | the `ValidationReport` JSON (the MS6 wire shape; `schemas/validate.schema.json`) |

`<path>` is the dataset root — the directory holding `manifest.json`.

## Exit codes

The exit code is derived **solely** from the verb's `Result`; the bin adds no contract
logic, only result→code routing.

| Code | Meaning |
|---|---|
| `0` | success — `describe` succeeded, **or** `validate` returned `conformant: true` |
| `1` | non-conformant — `validate` returned a report with `conformant: false` |
| `2` | usage / IO error — bad args, unreadable / nonexistent path, **malformed** manifest, or the §0 hard cut (unknown `format_version`) |

The load-bearing distinction (spec §0 vs §14): a `conformant: false` **report** — a
violated `MUST` that *ran* — is exit **1**, and is **distinct** from a structural / entry
**error** (exit **2**). The §0 hard cut (an unknown `format_version`) surfaces from the
verb as an `Err` and falls into the exit-2 arm; the CLI **never** softens it into a
`conformant: false` report. A non-conformant report (exit 1) is still a well-formed
report and still validates against `schemas/validate.schema.json`.

## Invariants

- **Thin glue only.** `main.rs` does exactly four things per subcommand: parse one path,
  call the verb, serialize the returned value as JSON, map the result to an exit code. No
  §14 rule, no manifest parsing, no reader, no discovery lives here.
- **JSON on stdout is output.** The verb's serialized JSON is printed to **stdout** and is
  the program's *output*, not a diagnostic. The wire shape is the MS5/MS6 serializer output,
  reused verbatim and never re-derived — the CLI honors `schemas/describe.schema.json` and
  `schemas/validate.schema.json` exactly (R4; asserted end-to-end in `tests/cli.rs`).
- **Diagnostics via `tracing` to stderr.** All diagnostics go through `tracing` to
  **stderr**; `println!` is used only to emit the JSON output value, never for diagnostics.
- **`hdx-core` is the home of all contract logic.** The CLI introduces no new spec
  `MUST`-check; it exposes the verbs' enforcement of the full §14 list through a stable
  surface (architecture §2).
