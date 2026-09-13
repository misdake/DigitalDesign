# DigitalDesign agent guide

Workspace map, dependency direction, prerequisites, and build/validate commands: `README.md` and
`ARCHITECTURE.md` (read `ARCHITECTURE.md` before moving code across crates).

**Read `.agent/README.md` first.** `.agent/` is a local, untracked working area (backed up outside
git): the conventions, the commit checklist, the todo list, the project journal, design drafts, and
the retired-document store. If it is missing, the tracked documents (`README.md`, `ARCHITECTURE.md`,
`ip/*/docs`, `systems/*/docs`) are authoritative. `<project>` below means the row with
`status: active` in `.agent/README.md`'s project table.

## Working loop

1. **Intake.** Read `.agent/README.md`, the open items in `projects/<project>/todo.md`, and the
   current `projects/<project>/project.md`.
2. **Work.** Honour the hard constraints, and run the mandatory co-simulations for every path you
   touch.
3. **New documents.** When the user says a document is new, file it as `.agent/README.md` describes:
   tracked → the matching `docs/` tree plus that directory's `README.md` table; local → under
   `.agent/` with `id`/`status`/`last-verified` front matter plus one index row. One fact lives in one
   document; fitted numbers are linked, never copied.
4. **Committing.** Read `.agent/commit-checklist.md` before and after a commit; it is the only place
   that spells out the two ledger rows (`scripts/log-post-commit-pnr.ps1`,
   `scripts/log-post-commit-benchmark.ps1`) and the diary.

Read on demand, never by default: `.agent/conventions.md`, `.agent/commit-checklist.md`,
`.agent/logs.md`. When a document's home, or whether a change counts as a finished unit, is
unclear, ask instead of deciding silently.

## Hard constraints

- `compiler/rcc` must not import either CPU crate (`scripts/check-layering.ps1` enforces it); CPU V3
  has its own `rcc_backend`. CPU V2 `src/isa.rs` and `src/isa.html` define ISA v2.6 and are frozen.
- Never check in a second hand-maintained instruction or Flash byte array: the CPU V3 build script
  generates the boot stage, applications, and boot image data from `systems/cpu-v3-tang-nano-20k/rcc`
  into Cargo `OUT_DIR`; use the `cpu-v3-boot-assets` binary to materialize those exact files.
- Verilog: a `?:` is unsigned when any branch is unsigned, so never nest `>>>` or a signed comparison
  in one — compute each signed result in its own assignment or a statement-based `case` and select
  between the results. `ASR`/`ASRI` (hence `fix16::to_int()`) broke on hardware this way; after any
  RTL signed-arithmetic change run both co-simulations below.

## Tool environment

Icarus Verilog: `IVERILOG_EXE`, `VVP_EXE` (the ignored co-sim tests and
`scripts/validate-hardware.ps1 -Mode iverilog|all` read them). Gowin: `GOWIN_HOME` or `--gowin-home`.

## Verification the agent must run

Beyond the baseline `cargo test` / `cargo clippy` / layer / hygiene / docs checks in `README.md`:

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
- Project files, comments, and documentation are English; local notes under `.agent/` may be Chinese.
- Rust changes stay `cargo fmt` clean (see `.agent/conventions.md` for the known pre-existing debt).
- `.agent/` is untracked and outside git: deleting a local document is permanent, so retire it into
  `.agent/history/` instead of removing it.
- Do not commit unless the user explicitly asks for a commit.
