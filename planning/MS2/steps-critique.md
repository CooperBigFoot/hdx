# MS2 STEP-PLAN CRITIQUE — adversarial review

**Target:** `planning/MS2/steps.md` (5 steps, S1–S5)
**Milestone:** MS2 — Fixture generator: one valid + two minimal invalid datasets (resolves R2, part 1)
**Contract:** `spec/HDX_SPEC.md`; planned against `architecture.md` §1/§2/§4/§7 (R2) and `planning/milestones.md` MS2.
**Repo state verified:** MS1 is built and green (`crates/core/src/{newtypes,error,format_version,field,manifest}.rs`), `schemas/manifest.schema.json` present and is exactly the six-field floor with `additionalProperties:false`, `conformance/` is empty (`.gitkeep` only), host Python is **3.14.3** (matches the plan's stated wheel-compat risk), `scripts/bump-version.sh` present, `Cargo.toml` at `0.1.12`.

**Verdict: APPROVED (severity: low).** Zero high/critical issues. Full deliverable + exit-criterion + spec-ref coverage. Correct ordering. Each step is independently committable and leaves the unchanged Rust crate green. Conventions honored. Both folded STEP-2 issues (MED-5, LOW-2) are genuinely incorporated, not cosmetic. The low/medium issues below are refinements the implementer should fold in; none blocks approval.

---

## What the plan gets right (verified, not assumed)

- **Scope is clean.** Zero Rust source change across all five steps; only `conformance/**`, `.gitignore`, and the mandated `Cargo.toml` bump/tag. No reader crate (`parquet`/`arrow`/`zarrs`/`tiff`/`geoarrow`) added. No verb (`describe`/`validate`), no §14 rule engine, no CLI — all correctly deferred. No `regrid`/`clip`/`reduce`/reduction/hydrology anywhere. Gridded fields are explicitly delineation-neutral over the bbox (§9), never pre-clipped. The "green check" is correctly redefined as "the unchanged Rust crate still builds/tests/clippies," which is the only honest meaning of green for a Rust-free milestone.
- **Inert/agnostic discipline is enforced at the fixture level.** `manifest.json` is exactly the six §11 floor fields (S2 line 245–249), matching the committed `schemas/manifest.schema.json` (verified: `additionalProperties:false`, six `required`). No content hash, no data-version, no field catalog, no transform/role/semantic/provenance key. The `{field}_was_filled` companion-mask and `{source}_{variable}` patterns are present *only as ordinary fields* with the explicit purpose of letting later milestones prove no special-casing — correct reading of §2.
- **MED-5 is a named hand-off, not an afterthought.** The plan dedicates a titled section ("The MED-5 writer/reader self-assertion hand-off"), names the two at-risk properties precisely (parquet `time` row-group statistics → MS3 Rust confirmation; Zarr v3 consolidated metadata → MS4 Rust confirmation), states the *regenerate-never-workaround* rule, and requires it to appear in **three** load-bearing places: a header comment on each at-risk write assertion (S2 line 274–276; S3 line 371–375), the README first version (S1 line 195–198), and a named README section in the finalized README (S5 line 504–506). This is the strongest possible folding of MED-5.
- **LOW-2 is enforced by construction, not just documented.** Each invalid is `copy(baseline) + exactly one mutation` (S4 line 433–439), and S4 adds an *invalid-side self-assertion* that the derived tree differs from the baseline in exactly the one intended way (recursive tree diff, S4 line 440–446). The hard rule "never hand-edit a fixture tree; add a mutation and regenerate" is stated in S1 README, enforced in S4, and finalized in S5. This is genuine incorporation.
- **Ordering is a valid topological sort.** S1 (harness) → S2 (scalar spine, establishes the `time` axis) → S3 (gridded half aligns the Zarr `time` to the already-written scalar `time`, and the COG/Zarr share one grid label) → S4 (invalids derive from the *complete* four-quadrant baseline) → S5 (README documents the *final* set + wires end-to-end regenerate). No step depends on a later step. The S3-after-S2 rationale (Zarr aligns to written scalar time) and S4-after-S3 rationale (invalids derive from the full baseline) are both correct.
- **Per-step green + committable.** The S2 intermediate tree (scalar + outlines, no gridded subtrees) is itself a *conformant scalar-only dataset* under §2/§5 (a dataset with no gridded fields has no `gridded_*` artifacts), so committing it does not bake in an L2/L3 violation; S3 then evolves the schema by adding gridded fields. This is internally consistent at every commit. The Rust gate (`cargo build`/`test`/`clippy --all-targets -- -D warnings`) is re-run as the green gate after every step.
- **Conventions honored.** Diagnostics routed through Python `logging`, not raw `print` (S1 line 188–190) — the correct analogue of the `tracing`-not-`println!` rule. Pinned deps + own venv for the 3.14 wheel-compat risk. Every step ends with `bump-version.sh patch` + stage `Cargo.toml` + conventional commit + `git tag v<version>`. Commit messages are conventional (`chore(conformance):`, `feat(conformance):`, `docs(conformance):`).
- **Coverage table is honest.** Every MS2 deliverable and exit criterion maps to a step. The "Seeds (not enforces)" labeling is consistently applied — MS2 ships no Rust and enforces no check; it engineers on-disk preconditions for L1/L2/L3/I1/I2/I3/H1/H2/T1/T2/G1/G2/G3/Geo1/M5/M6. The two invalids correctly pin exactly M2 and L1. The exhaustive one-per-check family is correctly left to MS8.

---

## Issues

### LOW-1 — `created_at` (and other writer metadata) is not pinned, yet "byte-deterministic" is an exit criterion and the LOW-2 tree-diff depends on it
**Step:** S2 (root cause), S4 + S5 (where it bites). **Category:** spec-drift / not-green (determinism).
The MS2 milestone reviewable outcome and S5 exit criterion both demand a **byte-deterministic / byte-identical** regenerate (`steps.md` line 520, 532; milestones.md MS2 reviewable outcome). But `manifest.json.created_at` is an RFC 3339 timestamp (S2 line 247). If the generator stamps wall-clock time, two `regenerate.sh` runs produce different `manifest.json` bytes and the S5 determinism exit criterion silently fails. The S4 "differs in exactly one way" self-assertion compares each invalid against *the baseline produced in the same run*, so it is robust to a per-run timestamp; but the milestone's own byte-identical-across-runs claim is not. Secondary nondeterminism sources to pin: pyarrow's parquet `created_by`/key-value metadata (deterministic only because deps are pinned — make this explicit), any GeoTIFF software tag, geoparquet `geo` metadata, and file mtimes if the diff is mtime-sensitive.
**Suggested fix:** In S2, fix `created_at` to a constant literal (e.g. `"2026-01-01T00:00:00Z"`) and state in the generator + README that *all* fixture inputs (timestamps, RNG seeds for any field values, writer metadata) are pinned so the tree is byte-reproducible. In S5, make "re-running yields a byte-identical tree" a concrete, run-twice-and-diff self-check, not just prose.

### LOW-2 — `Makefile` target is "MAY", but milestones.md lists "`Makefile`/`regenerate.sh`" and the plan should not leave the choice ambiguous in an exit criterion
**Step:** S1 / S5. **Category:** vague-acceptance (minor).
milestones.md MS2 deliverables say "A `Makefile`/`regenerate.sh` target rebuilds all fixtures deterministically." The plan commits firmly to `regenerate.sh` and treats `Makefile` as optional ("A `Makefile` target `regenerate` MAY wrap it", S1 line 186). That is a defensible reading (the slash is an either/or), so this is not a coverage gap — but the acceptance text should state plainly that `regenerate.sh` *is* the canonical deterministic rebuild target satisfying that deliverable, so a reviewer does not expect a `Makefile`.
**Suggested fix:** One sentence in S5 acceptance: "`regenerate.sh` is the canonical rebuild target; no `Makefile` is required (the milestones.md slash is either/or)."

### LOW-3 — "deliberately broken variant aborts" is described as a manual/local check, not a committed regression
**Step:** S2 (line 285–288), S5 (line 522–523). **Category:** vague-acceptance.
The plan proves the self-assertions are load-bearing by *locally* breaking a property (e.g. unsorted `time`) and confirming generation aborts, then reverting — explicitly "run manually/locally, not committed." That is acceptable for MS2 (committing a broken fixture would violate scope), but "I tested it locally" is not reproducible by the next agent. Since the generator is Python and ships in `conformance/generator/`, a tiny committed Python unit test that feeds each assertion a deliberately-malformed in-memory input and asserts it raises would make the load-bearing claim durable without committing any broken on-disk tree.
**Suggested fix:** In S5 (or S2), add a committed generator-side unit test (`conformance/generator/tests/`) that exercises each self-assertion's failure path on synthetic bad input, so "assertions abort on failure" is a re-runnable fact, not a manual ritual. (Keeps Rust untouched; still green.)

### LOW-4 — `missing-root-rollup` picks "`scalar_static.parquet` *or* `outlines.geoparquet`" — the choice must be fixed and recorded, or the fixture is nondeterministic and L1's pinned meaning is ambiguous
**Step:** S4 (line 437–439). **Category:** vague-acceptance / spec-drift (minor).
S4 says delete "`scalar_static.parquet` *or* `outlines.geoparquet`." Leaving the choice open means the fixture is not deterministic and a downstream MS6/MS8 author cannot know which file is absent. The plan does say "Document which rollup is removed in the README check-id table" (line 439, and S5 line 508), which mostly closes this — but the *step* should commit to one specific rollup now (both pin L1 equally; pick one and freeze it).
**Suggested fix:** In S4, pick one (e.g. delete `outlines.geoparquet`) and state it as the fixed mutation; the README table then documents that exact choice. Remove the "or."

### LOW-5 — confirm pyarrow can be *forced* to emit `time` row-group statistics on the chosen interpreter before relying on it as a seeded precondition
**Step:** S2 (line 274–276). **Category:** not-green (risk surfacing), already partly mitigated by MED-5.
The whole T1/§8 seed rests on pyarrow writing usable min/max row-group statistics for the timestamp logical type. The MED-5 hand-off correctly says MS3 confirms from Rust and a mismatch is fixed by regenerating. Good. But MS2 should still *fail its own self-assertion loudly* if pyarrow on the pinned interpreter cannot be coerced to write `time` statistics (rather than silently writing a file that MS3 later discovers is statless). The plan's S2 self-assertion already re-opens the file and reads row-group statistics — so this is mostly covered; the refinement is to make the abort message name the MED-5 hand-off and instruct "adjust writer settings and regenerate," so a future agent hitting a statless write is pointed at the fix, not left guessing.
**Suggested fix:** Ensure the S2 `time`-statistics self-assertion's abort message explicitly references the MED-5 regenerate-not-workaround rule (the header comment already does; mirror it in the runtime abort text).

---

## Folded-issue verification (STEP-2)

| Folded issue | Incorporated? | Where | Cosmetic? |
|---|---|---|---|
| **MED-5** writer/reader self-assertion linkage (named hand-off; parquet `time` stats → MS3 Rust, Zarr consolidated metadata → MS4 Rust; mismatch ⇒ regenerate, never reader workaround) | **Yes** | Dedicated titled section (lines 73–95); S2 self-assertion header comment (274–276); S3 self-assertion header comment (371–375); S1 README (195–198); S5 named README section (504–506); coverage row (576) | **No — genuine**, appears as runtime assertion + source comment + README section in three steps |
| **LOW-2** derived-not-hand-authored (one surgical mutation each from one baseline; hard rule in README; no hand-editing) | **Yes** | Dedicated titled section (99–109); S4 copy+one-mutation derivation (433–439) enforced by "differs in exactly one way" self-assertion (440–446); S1 README states it (194–195); S5 finalizes it (501–503); coverage row (577) | **No — genuine**, enforced *by construction* at generation time, not merely documented |

Both folded issues are incorporated as load-bearing mechanisms (runtime self-assertions + source comments + README hard rules), not as passing mentions.

---

## Coverage ledger (deliverable / exit criterion → step → confirmed)

- Pinned Python project under `conformance/generator/` → S1 ✓
- Deterministic `regenerate.sh` rebuild target → S1 stub → S5 end-to-end ✓ (caveat LOW-1 determinism)
- Dev-only / not-a-writer rule in README → S1 → S5 ✓
- `valid/minimal/` ≥2 basins, all four quadrants → S2 (scalar·static + scalar·dynamic) + S3 (gridded·static + gridded·dynamic) ✓
- COG + Zarr share one grid label, cell-for-cell aligned (G2 precondition) + self-assertion → S3 ✓
- Ragged-across-basins (§6.1) + aligned-within-basin (§6.2) time + self-assertions → S2 + S3 ✓
- In-file `basin_id` == folder + unique (I2/I3) + self-assertion → S2 ✓
- `time` full timestamp, non-null, sorted (T1) + row-group statistics (§8) + self-assertion → S2 ✓
- Zarr consolidated metadata + v3 sharding (§8) + self-assertion → S3 ✓
- Companion-mask + `{source}_{variable}` ordinary fields + self-assertion → S3 ✓
- `outlines.geoparquet` (basin_id, delineation, geometry), ≥2 labels, not partitioned (Geo1) → S2 ✓
- `invalid/wrong-format-version/` (M2), one mutation → S4 ✓
- `invalid/missing-root-rollup/` (L1), one mutation → S4 ✓ (caveat LOW-4: fix which rollup)
- README: layout + regenerate + dev-only + check-id table → S5 (+ S1 first version) ✓
- Self-asserts every engineered property; failure aborts → S2/S3/S4 per-property → S5 end-to-end abort ✓ (caveat LOW-3: make the "abort proven" a committed test)
- Seeds L1,L2,L3,I1,I2,I3,H1,H2,T1,T2,G1,G2,G3,Geo1,M5,M6 → S2+S3 → S5 seeding table ✓
- No Rust build change; crate stays green → S1–S5 ✓
- Bump+tag discipline → S1–S5 ✓
- Inert/agnostic; six-field floor; delineation-neutral grids → S1–S5 ✓
- MED-5 hand-off → S2 + S3 + S5 ✓
- LOW-2 hard rule → S1 + S4 + S5 ✓

No deliverable, exit criterion, or spec ref is unassigned. No step exceeds MS2 or performs a later milestone's work.

---

## Final

Five low-severity refinements, zero medium/high/critical. Coverage is complete, ordering is a correct topological sort, each step is independently committable and leaves the Rust crate green, conventions are honored, and both STEP-2 folded issues (MED-5, LOW-2) are genuinely and durably incorporated. **Approved.** The implementer should fold LOW-1 (pin `created_at` and assert byte-reproducibility) and LOW-4 (fix which rollup is deleted) as they are the two that touch reproducibility correctness; LOW-2/3/5 are polish.
