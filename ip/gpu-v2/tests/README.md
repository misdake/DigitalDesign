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

`texture_registered_return.py` compares a current physical Group35 command export
with an approved baseline. It exercises native/portable cache banks, all64 local
origins, conflicting ways, CE and commit denial, refill stalls and held results.
The 384 returned payload/texel records must match exactly; each simulation has
a 30,000-cycle and 60-second bound.
