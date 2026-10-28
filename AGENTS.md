# DigitalDesign agent guide

Workspace map, dependency direction, prerequisites, and build/validate commands: `README.md` and
`ARCHITECTURE.md` (read `ARCHITECTURE.md` before moving code across crates).

**Read `.agent/README.md` first.** `.agent/` is a local, untracked working area (backed up outside
git): conventions, the commit checklist, project goals, open tasks, specifications, and daily/weekly
logs. If it is missing, the tracked documents (`README.md`, `ARCHITECTURE.md`, `ip/*/docs`,
`systems/*/docs`) are authoritative. `<project>` below means the current project directory reached
through `.agent/projects/README.md`.

## Working loop

1. **Intake.** Read `.agent/README.md`, the open items in `projects/<project>/todo.md`, and the
   current `projects/<project>/project.md`.
2. **Design first.** Develop the top-level, module and software designs in
   `.agent/projects/<project>/design/<topic>/`, outside Git. Include the target specification,
   changes from implemented behavior, implementation approach and acceptance conditions. Maintain
   the project's todo throughout. Design documents contain pending work, not development logs.
3. **Implement incrementally.** Once the project design is ready, read `.agent/conventions.md`
   for the development, integration and delivery workflow. After each part is implemented and
   verified, merge its actual specification and implementation description into the owning tracked
   `docs/`, remove that completed part from local design, and update todo. The user backs up the
   design process. Honour the hard constraints and run the mandatory validation for each
   implementation path touched.
4. **Document ownership.** `docs/` describes implemented interfaces, formats, behavior, storage,
   scheduling and brief tradeoffs. Pending specifications stay in local design. Maintain each
   document list only in its own directory's README; local documents have
   `id`/`status`/`last-verified` front matter. Do not maintain a global document list or parallel
   copies of an implemented specification in design and docs.
5. **Committing.** Read `.agent/commit-checklist.md` before and after a commit. Keep daily and weekly
   logs current. Historical performance and resource records remain in the user's backup; do not
   recreate them, append ledger rows, or run legacy logging scripts. Required implementation
   validation still applies.

Read `.agent/commit-checklist.md` and `.agent/logs.md` on demand. When a document's home, or whether
a change counts as a finished unit, is unclear, ask instead of deciding silently.

## Worktree lifecycle and integration

- Developer roles persist; feature worktrees are temporary. Reuse a suitable existing feature
  branch/tree, and do not create a worktree for read-only analysis.
- DigitalDesign linked worktrees belong under `E:\worktrees\DigitalDesign`; other projects belong
  under `E:\worktrees\codex-others`. Do not create project worktrees in Codex task directories.
- One maintainer serializes integration into `code-v0.3`. Use fast-forward integration for small
  qualified changes and explicit merge boundaries for substantial multi-commit features.
- Update private branches against accepted canonical dependencies before integration, then run
  affected regression checks. Avoid repeated cherry-picks of shared fixes; do not rewrite shared
  history without authorization.
- After accepted integration, preserve recovery refs and unique evidence, confirm the owner is
  idle, and close redundant clean checkouts. Delete temporary branches only after confirming
  mainline inclusion. Preserve the `code-v0.*` version branches.
- Active builds retain their caches. Authorized cleanup of paused work must first preserve unique
  evidence outside every disposable `target` directory, with source identities and SHA mappings.
  Use project evidence storage such as `E:\worktrees\DigitalDesign\evidence`.
- Confirm a tree is unused before moving it. Preserve HEAD, index, WIP and untracked files; prefer
  Git worktree operations and verify file hashes after migration. Never overwrite a destination
  or discard unique work. Local housekeeping does not authorize remote pushes.

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
- `.agent/` is untracked: local cleanup requires user authorization. Preserve and continue daily
  and weekly logs; keep specifications separate from development history.
- Do not commit unless the user explicitly asks for a commit.
