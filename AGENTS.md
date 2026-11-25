# DigitalDesign agent guide

`README.md` describes the workspace and build and validation commands. Read `ARCHITECTURE.md`
before moving code across crates. Keep implemented interfaces and behavior in the owning crate's
`docs/` and maintain its document index in that directory's README.

Work directly on the user's assigned task. Check the current branch and preserve existing work.
Reuse a suitable checkout for implementation and use the current checkout for read-only analysis.
Follow the user's branch and integration instructions. Verify inclusion and preserve unique work
before authorized cleanup of temporary branches, worktrees and build evidence.

## Hard constraints

- `compiler/rcc` must not import either CPU crate (`scripts/check-layering.ps1` enforces it); CPU V3
  has its own `rcc_backend`. CPU V2 `src/isa.rs` and `src/isa.html` define ISA v2.6 and are frozen.
- `compiler/rcc` targets CPU V3 only. CpuV2 compatibility is **not** an acceptance criterion for new
  work: the V2 backend and its existing tests stay as they are, a new frontend feature needs to be
  correct (and tested) on CPU V3 only, and a feature that happens to work on CpuV2 is a bonus rather
  than a requirement.
- Never check in a second hand-maintained instruction or Flash byte array: the CPU V3 build script
  generates the boot stage, applications, and boot image data from `systems/cpu-v3-tang-nano-20k/rcc`
  into Cargo `OUT_DIR`; use the `cpu-v3-boot-assets` binary to materialize those exact files.
- Verilog: a `?:` is unsigned when any branch is unsigned, so never nest `>>>` or a signed comparison
  in one — compute each signed result in its own assignment or a statement-based `case` and select
  between the results. `ASR`/`ASRI` (hence `fix32::to_int()`) broke on hardware this way; after any
  RTL signed-arithmetic change run both co-simulations below.

## Tool environment

### Tang Nano 20K board access

Use the requested target's audited artifact and record its image identity and
physical results. Board procedures are in `hardware/vendor/gowin/scripts/README.md`.

Icarus Verilog: `IVERILOG_EXE`, `VVP_EXE` (the ignored co-sim tests and
`scripts/validate-hardware.ps1 -Mode iverilog|all` read them). Gowin: `GOWIN_HOME` or `--gowin-home`.

## Verification the agent must run

Run Cargo tests for the affected packages and test targets. Skip workspace-wide test runs,
including validation scripts that invoke `cargo test --workspace`. Use the Clippy, layering,
source hygiene, and documentation checks described in `README.md`.

### Required co-simulations

- CPU V3 emulator-vs-RTL co-simulation (pipeline, forwarding, retirement, data/handshake):
  ```powershell
  & scripts/run-cargo.ps1 -Subcommand test -Label "cpu-v3 emu/rtl co-sim" -CargoArgs @("-p", "cpu-v3", "--lib", "--", "--ignored", "--nocapture", "--test-threads=1")
  ```
- System-level co-simulation (core, fetch, cache, arbiter, memory):
  ```powershell
  & scripts/run-cargo.ps1 -Subcommand test -Label "cpu-v3 system co-sim" -CargoArgs @("-p", "cpu-v3-tang-nano-20k", "--test", "system_cosim", "--", "--ignored", "--nocapture", "--test-threads=1")
  ```

## House rules

- Every simulator test must supply a maximum cycle/step count.
- Write project files, comments, and documentation in English.
- Keep Rust changes `cargo fmt` clean.
- Keep historical performance and resource records in the user's backup. Use current validation
  commands for new measurements.
- Do not commit unless the user explicitly asks for a commit.
