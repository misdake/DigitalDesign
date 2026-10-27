# GPU regression tests

Pure host regressions share the `host` integration executable. Their component
modules remain independently filterable, for example:

```powershell
cargo test --release -p gpu-v2 --test host lighting_scalar::
cargo test --release -p gpu-v2 --test host texture_coordinate::
```

RTL/qualification tests keep their independent integration targets and explicit
ignored-test entry points. Tests whose private fixture depends on its original
crate-root context also remain separate. Shared host fixtures are loaded once
by `host.rs`, rather than compiled independently in every component module.

The lighting corpus uses pairwise normal/position/direction combinations and
shininess endpoints. Dedicated scalar/system checks retain every shininess code,
signed and prescale boundaries, degenerate half vectors, normal/light
cancellation and fixed-seed random/highlight samples. All stage equality and
error limits remain assertions. Dense historical characterization sweeps are
not part of routine regression, and the resulting sample reports are not new
exhaustive numerical qualifications.

The framebuffer-cache protocol tests retain all six independent cases, complete
byte-image checks, CE pauses, beat backpressure and transport/compute failures.
They compile once per geometry within each test process and replay independent
input files through that executable. Geometry, buffer limits and the finite
watchdog are checked before each replay; no compiled artifact survives as a
trusted cache across runs.

The ignored `texture_registered_return` Rust target exports the current command
cache and connects its native DO directly to the production two-edge Color
consumer. Eight bounded cases check native/portable RAM, one/two mip groups,
447 consecutive hit intervals at II2, per-owner weights and keyed RGB results.
Pressure cases combine CE pauses, consumer stalls, refill gaps and conflicting
ways; they also check held Q and read/refill line exclusion. Each simulation is
limited to 30,000 clocks and each process to 45 seconds.

`texture_registered_return.py` compares a current physical Group35 command export
with an approved baseline. It exercises native/portable cache banks, all64 local
origins, conflicting ways, CE and commit denial, refill stalls and held results.
The 384 returned payload/texel records must match exactly; each simulation has
a 30,000-cycle and 60-second bound.

The ignored `texture_registered_return::packed_coordinate_to_color` test connects
the production coordinate, coefficient, membership, cache and Color modules.
It checks 512 keyed, integer-reference pixels per case for bilinear, fractional
trilinear and magnification, with CE pauses. Resident one-mip traffic must sustain
II2; two-mip traffic must sustain II4. Each process is bounded to45 seconds and
each simulation to30,000 clocks. `vliw_backend_complete.py --mask-matrix` separately
checks all16 masks through the real producer pool and the complete backend:
exact UV-read conservation, no helper reads, actual first/last lane ownership,
Final/ROP results and the entire memory image, in portable and native RAM modes.

The production pipeline probe also covers a four-tile fine footprint and its
five-Group trilinear counterpart: their hit intervals remain8 and10 clocks.
The Group-Q lifecycle probe checks256 distinct35-bit words through four address
wraps, with native/portable RAM, CE pauses, all48 FIFO credits reached, held
current/registered native heads, in-flight reads, exclusive Q return/promotion,
next-key order and concurrent writes/reads. Its simulation
is bounded to10,000 clocks. `group_lookahead_overlaps_next_tag` joins the actual
Group queue and cache:96 keyed Groups check resident hits at II2 and consecutive
misses at20 clocks with a fixed16-beat memory peer, in both RAM modes. This is a
bounded cache-schedule regression, not a physical MC throughput measurement.
`direct_lod_stage1.py --production-source <gpu.v>` checks all mip sizes and
filters against integer coordinate/owner gold while honoring the producer's
U/V Q lifetime. `vliw_backend_complete.py --nearest` uses a nonuniform texture
oracle for nearest filtering; it does not substitute the trilinear reference.

`texture_registered_return::tile_crossings_preserve_nonuniform_texels_and_single_mip_quad_rate`
uses the production coordinate-through-Color path with sector-dependent RGB565
texels and an independent integer address/mip/weight/color reference. It checks
actual lambda0 versus256, fine/coarse member conservation, single-mip bilinear
and true trilinear,25%Xedge/25%XYcorner mixes, saturated rates and complete quads
offered every16clocks. Both single-mip filters must retain16clocks/quad in the
warm window; two-mip rates reflect their actual additional Groups. Output records
separate first latency, steady intervals, final drain and warm/cold queue peaks.
Each case is bounded to512pixels,30,000clocks and45seconds. The16-beat memory peer
is not a physical-MC or full-GPU performance model.

The lighting stream regression simulates the complete clean and CE-stalled
64-pixel streams independently. Numerical/date/overflow audit faults reuse the
accepted clean result, restoring the first record before each mutation; they do
not repeat identical arithmetic simulation. The input corpus, context switches,
II assertions, numerical oracle and watchdog checks are retained.
