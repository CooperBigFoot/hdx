"""HDX conformance fixture generator (dev-only harness).

This package builds the on-disk conformance fixtures (one valid + two minimal
invalid datasets) that the Rust ``hdx-core`` readers and verbs are tested
against. See ``conformance/README.md`` for the three load-bearing rules.

THREE LOAD-BEARING RULES (full text in conformance/README.md):

1. Dev-only / NOT an HDX writer. This package lives only under
   ``conformance/generator/``. It is never shipped in, imported by, or linked
   from ``hdx-core``. There is no HDX writer in v0.1 (spec §10 / architecture §7
   R2); this harness only *emits bytes a reader will later read*, and never
   defines or executes any contract logic.

2. LOW-2 — derived, not hand-authored. Every invalid fixture is produced
   programmatically from the single valid baseline via exactly one surgical
   mutation. Fixture trees are NEVER hand-edited; a contributor adds a mutation
   here and regenerates.

3. MED-5 — Rust-side confirmation hand-off. Two engineered properties (parquet
   ``time`` row-group statistics; Zarr v3 consolidated metadata) are asserted
   here on the *writer* side only. MS3 (parquet stats) and MS4 (Zarr metadata)
   MUST confirm them from the Rust reader side. A writer/reader mismatch is
   fixed by REGENERATING the fixture, never by a reader workaround.

Diagnostics go through the standard :mod:`logging` machinery (see
:data:`logger`), not raw ``print`` — mirroring the architecture §2 split between
diagnostics (logging, to stderr) and user-facing output.
"""

import logging
import os

__all__ = ["__version__", "logger", "get_logger"]

__version__ = "0.1.0"

# Module-level logger. A single NullHandler keeps the library import side-effect
# free; the CLI entry point (configure_logging) attaches a real handler. This is
# the standard "library configures nothing; application configures handlers"
# pattern.
logger = logging.getLogger("hdx_fixtures")
logger.addHandler(logging.NullHandler())


def get_logger(name: str | None = None) -> logging.Logger:
    """Return the package logger, or a child logger ``hdx_fixtures.<name>``."""
    if name is None:
        return logger
    return logger.getChild(name)


def configure_logging(level: int | str | None = None) -> None:
    """Attach a stderr stream handler to the package logger (idempotent).

    The level defaults to the ``HDX_FIXTURES_LOG_LEVEL`` environment variable, or
    ``INFO`` when unset. Calling this more than once does not stack handlers.
    """
    if level is None:
        level = os.environ.get("HDX_FIXTURES_LOG_LEVEL", "INFO")

    # Avoid duplicate stream handlers on repeated calls.
    has_stream = any(
        isinstance(handler, logging.StreamHandler)
        and not isinstance(handler, logging.NullHandler)
        for handler in logger.handlers
    )
    if not has_stream:
        handler = logging.StreamHandler()  # defaults to stderr
        handler.setFormatter(
            logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
        )
        logger.addHandler(handler)

    logger.setLevel(level)
