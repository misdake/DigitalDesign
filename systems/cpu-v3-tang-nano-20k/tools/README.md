# System software tools

- `native_asset_source.py` writes immutable source declarations and typed native
  Texture/Mesh/UI-atlas descriptions from producer metadata. It leaves addresses
  to placement and emits sector fixups for address-independent mesh packages;
  see [static assets](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md).
- `gpu_mesh_layout.py` owns the shared V96/T64 meshlet record encoder;
  `gpu_texture_layout.py` owns native 16-bit texture packing, including mip tails.


- gpu_layout.py reads rcc/gpu_layout.rs for asset producer bounds.
  GPU addresses have one authority shared by RCC, Rust boot packing and Python
  asset producers; the reader rejects unsupported expressions and overflow.

- `instance_order_search.py` runs bounded instance declaration order experiments
  on an audited exported Gowin project. It never changes source-file ordering,
  functionality, parameters, initialization, clocks, pins or timing constraints.
  The generator currently owns a raw Verilog template rather than a structured
  instance list, so this tool moves only twelve explicitly named complete blocks
  in the first module. It preserves all other bytes and places those blocks after
  all existing declarations/behavior; fixed helper modules remain untouched.

  Before searching, retain the original twelve-instance order in that final
  region and validate the normalization. A required `--gates` JSON receipt binds
  `source_commit`, `original_inputs` and `normalized_inputs` (relative path to
  SHA-256 maps for all exported `src` files), `connection_sha256`, and `passed` to
  the relevant simulations. `stage_started_at` optionally includes earlier pilot
  runs in the same time budget. The receipt is evidence of independently run
  tests, not a substitute for running them.

  Every candidate must preserve complete instance bodies and non-instance bytes,
  elaborate with strict net declarations in the changed module, and match the
  normalized baseline's declared signals, widths, directions, module hierarchy
  and named-net nexus aliases. Compiler-generated local expression names and
  memory addresses are excluded from that comparison; unchanged source bodies
  protect the expressions themselves. SDK primitive diagnostics are retained
  separately. Project implicit-net/port-width diagnostics, new synthesis width
  diagnostics, missing instances or additional module trimming invalidate a
  candidate. Frozen baseline expression/primitive-parameter truncation messages
  are recorded by exact signature and count; they are not port mismatches.

  Example (all output must be below this worktree's `target`):

  ```powershell
  python systems/cpu-v3-tang-nano-20k/tools/instance_order_search.py `
    --base target/audited-export --output target/instance-search `
    --gates target/normalization-gates.json --workers 2 `
    --max-candidates 16 --minutes 90 --stagnation 8 --seed 20261029
  ```

  `--workers` accepts 1, 2 or 4; its default remains 2. Four workers permit
  at most four candidate build jobs at once, so a16-candidate round can use four
  batches instead of eight when runtimes and the wall budget permit. This is a
  scheduler limit, not a statement about available RAM, CPU threads or Gowin
  license capacity. Each Gowin process may itself use multiple threads. Confirm
  the complete-system baseline's synthesis/PnR peak, host headroom, tool/license
  availability and other owners' physical workloads before selecting four.
  Resource or licensing failures must be reported; do not silently change worker
  count, reset the deadline or start another round. Worker count is part of the
  immutable checkpoint context. The round's candidate limit still applies to
  all slots together, and the independent best-candidate reproduction runs only
  after candidate jobs drain, within the same final20-minute reserve.

  The four initial orders are original, GPU first, GPU last and reversed. A pool
  of at most six routed, structurally valid parents retains the current best,
  area/margin frontier extremes, a near-feasible timing case and permutation
  diversity. Pool priorities provide descending parent weights; 15% uniform
  exploration keeps every retained parent eligible. Move/swap neighbours and,
  after stagnation, bounded three-edit perturbations escape local repetition.
  Timeout/unrouted cases have no invented slack and do not become timing parents.
  Completion-first scheduling
  registers each finished job and immediately refills its slot, with at most the explicitly selected one, two or four
  active jobs. Each proposal, random state and actual completion order is saved.
  A fixed seed reproduces proposals for a given recorded history; asynchronous
  runtimes can change completion order in a fresh run. Invalid candidates never enter
  ranking or the portfolio. Legal fully routed setup/hold-clean candidates rank
  by Logic then setup slack. Unqualified routed candidates rank by worst negative
  setup/hold slack, then the unique negative endpoint sum in the printed path
  tables, then Logic. Table coverage is recorded: a partial sum is not full TNS.
  Gowin's per-clock TNS is retained separately because it can omit cross-clock
  violations. Unrouted cases rank after routed cases.

  Each job has its own inputs, compiler outputs, effective options, synthesis
  project, PnR command, full logs, reports and hashes. Input/permutation caching
  does not claim netlist equivalence: `.vg` hashes are audit identities only.
  `--import-result` may reuse an earlier pilot only after checking its actual
  inputs, build script and elaborated connections. Re-run the exact same command
  to resume the locked atomic checkpoint and completed-job cache. Incomplete jobs
  are preserved; wait for any recorded PID before a single fresh retry. Changed
  inputs, settings or tool script require a new explicit stage directory.
  An explicit `--scheduler-update` is the sole exception: it requires the previous
  exact script in `frozen-flow.py`, verifies unchanged module constants and all
  non-scheduler functions by syntax tree, and preserves the cache identity,
  pending orders, random state and original deadline. An already-running job
  occupies its slot until its recorded adoption monitor writes a result.
  An explicitly authorised `--policy-update --previous-tool <exact-old-script>`
  may additionally change only parent retention/proposals. Candidate validation,
  build, result extraction, fitness and constants must still match by syntax tree;
  the migration records both script identities and preserves the same checkpoint.

  The candidate, stagnation and wall budgets all stop the stage. Each job is
  limited to 20 minutes, and the last 20 minutes of the wall budget are reserved
  for an independent fresh synthesis/PnR of the best qualified candidate. No
  source template is automatically changed. A winner still needs source export
  identity checks, affected regression, a fixed commit and independent review.
  Continue with another stage only after reporting the current result.

  Fast tool tests:

  ```powershell
  python -m unittest discover -s systems/cpu-v3-tang-nano-20k/tools -p test_instance_order_search.py
  ```
# Application and hardware workflows

Use `cpu-v3-app preview APPLICATION` for a bounded first frame or `run APPLICATION`
for a window with input. Both share the ordinary build authority; `--snapshot`
consumes an existing export. See [commands and diagnostics](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md).
Build without running with `build-application.ps1`; its [artifact](/systems/cpu-v3-tang-nano-20k/docs/application-build-artifact.md)
is the input to `profile.py run|plan|report|compare`. Backend, selected frame and
budget are explicit; see [application profiling](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md).
`hardware.py audit|assemble|verify` consumes frozen fits and application snapshots;
only its `program-sram|program-flash` commands access a device. See the
[Flash workflow](/systems/cpu-v3-tang-nano-20k/docs/flash-layout.md).
