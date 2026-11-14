# Frozen Gowin hardware artifacts

Project export writes `gowin-project.json` alongside the generated files. Its
versioned identity binds the target/device/programmer metadata, source closure,
resource allocations and ownership labels, capacity and DSP/BSRAM expectations.
Static component names remain borrowed in live reports; deserialized reports
own those names through `Cow<'static, str>` without leaking strings.
Gowin synthesis uses `VerilogProject::generate_for_synthesis`: it validates the
same source signatures, dependency closure and resource ownership without
constructing simulation fixtures. `VerilogProject::generate` and explicit
simulation retain the required-testbench check; synthesis does not constitute
that qualification.

A successful build writes `gowin-build.manifest` after the timing/resource audits
and `gowin-fit.json` last. The latter binds the project snapshot, legacy build
manifest, exact FS, PnR resource/timing reports and synthesis resource XML. Reads
verify file identities and repeat the existing timing, fitted resource and
hierarchical ownership checks. Hash agreement alone is not a passing audit.

New fits also bind the complete warning evidence. The [warning audit](gowin-warnings.md)
classifies actual mapped primitive ports and bounded source-owned exceptions;
unknown diagnostics fail new-build qualification. Existing-artifact inspection
returns a compact warning summary and any remaining unknowns, including explicit
incomplete coverage for older fits.

```powershell
cargo run --release -p digital-design-hardware-gowin --bin gowin-artifact -- target/hardware
```

This standalone consumer has no CPU/System/application dependency and never
instantiates or generates a hardware Module. Audit needs no installed Gowin
tools, writes no files and does not compare an artifact with today's source.
Source locators must stay inside the snapshot, including after symlink resolution.
Wrong targets, missing closure files and changed source/report/FS bytes fail.

The project CLI separates these operations:

| Operation | Source behavior |
| --- | --- |
| Export / `--build` / `--program` | Explicit current-source generation and export; build/program as requested. |
| `--check-existing` | Load and audit frozen source and fit; no export. |
| `--audit-current` | Generate current source in memory, compare existing files, audit existing reports; no export. |
| `--seal-existing` | Explicitly qualify a legacy build against current source, then write its snapshot/fit seal. A stale or failing build cannot be migrated. |
| `--program-existing` / `--program-flash OFFSET FILE` | Audit frozen hardware first, discover installed programming tools only for the explicit device operation. |

Select one programming operation. The standalone tool accepts only existing
artifacts, including explicit SRAM or raw-offset Flash programming; it cannot
build or migrate a legacy fit. System deployment owns its application identity,
typed Flash mappings and complete-image composition above this vendor boundary.
An offline audit is not physical board qualification.
