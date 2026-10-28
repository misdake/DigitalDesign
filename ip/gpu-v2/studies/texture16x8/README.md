# Texture16x8 cache experiment

This opt-in experiment starts from accepted sampling commit
`8b242ec83a228b31f384ba703941271ef1d98313`. It uses external16x16 RGB565 tiles
and resident16x8 cache lines, matching the existing framebuffer's storage and
half-tile boundaries. The default `command` export and board factories retain
their accepted format. The layout-only profile exposes no framebuffer view.
The [pure column profile](/ip/gpu-v2/studies/texture-column16/README.md) changes
external macro order; RTT work remains separate from this layout feature.

## Storage and cache contract

Each line contains256 bytes and fills through two separately acknowledged128B
requests. The main configuration keeps8KiB data:32 lines,8 sets,4 ways, four
1024x16 data banks. The baseline has64 lines,16 sets,4 ways and the same8KiB.
The retained native weight/tag primitive allocation is unchanged.
Framebuffer lines additionally contain depth; their resident count is not a
texture-cache capacity reference.

For a mip base B, tile-row pitch P and texel coordinates x,y:

```text
T = (y >> 4)*P + (x >> 4)
line_address = B + 512*T + 256*((y >> 3)&1)
texel_address = line_address + 32*(y&7) + 2*(x&15)
request_address = line_address + 128*sector, sector=0,1
```

The physical256B line number is15 bits over the current8MiB address domain.
Together with7 localXY bits it preserves Group35, the48-credit FIFO, copied
head, registered native lookahead and paid-Q/Color lifetime. Bank selection is
`{y[0]^x[1],x[0]}`; its local row is `{y[2:0],x[3:2]}`. Every2x2 footprint still
has one texel per bank. Cross-line groups split at X15/Y7 rather than X7/Y7.

A parent line remains Filling across both sector requests. Only successful
completion of the second sector publishes Ready. A failed first or second
sector cannot publish the line. Already presented requests and accepted
returns drain across CE0, abort and downstream stalls. The next sector can
progress without compute CE. No additional miss or response FIFO is added.

All mip levels own complete16x16 tiles. Small mip texels repeat periodically
through padding. Prefixes in128B sectors are
`0,4,8,12,16,20,36,100,356,1380,5476`. A64-square full chain occupies100 sectors
instead of88. The experiment repacks simulation assets; existing boot assets
must not be used with `command16x8` unchanged. Bases must be256B aligned.

The texture descriptor remains square POT.400x240 width/height, pitch25,
boundary rules, RTT views and mip generation are separate work. Ordinary
physical-address content updates still require a drained texture invalidate;
framebuffer FLUSH does not invalidate the texture cache.

## Reproduction

Build the exporter and independent numerical reference through
`scripts/run-cargo.ps1`, then materialize the current MC peer:

```powershell
& scripts/run-cargo.ps1 -Subcommand build -Label texture-tools -CargoArgs @('-p','gpu-v2','--bin','gpu-v2-vliw-export','--example','vliw_integration')
& scripts/run-cargo.ps1 -Subcommand test -Label texture-mc -CargoArgs @('-p','gpu-v2','--test','host','vliw_sdram::export_real_shared_sdram_peer','--','--nocapture')
target/release/gpu-v2-vliw-export.exe target/texture-layout/baseline.v command
target/release/gpu-v2-vliw-export.exe target/texture-layout/candidate.v command16x8
python -B ip/gpu-v2/tests/test_texture16x8.py --export target/texture-layout/candidate.v --output target/texture-layout/rtl-tests
```

Run `texture_layout_replay.py` with `--layout 8x8` or `--layout 16x8`, the
corresponding export, a fresh `--output`, `--gold`, `--peer`, `--camera-index 0`
and `--native`. It records the complete tracked demo's24 meshlets and1141
triangles, including all948 rabbit faces, and runs the real GPU/MC/pin peer.
The independent numerical and whole-memory audit remains mandatory. A frozen
content-addressed reference executable prevents concurrent Cargo rebuilds
from changing oracle identity during a long simulation. `--audit-existing`
can repeat the complete audit only after matching DUT, observer TB, MC,
command and fixture inputs byte-for-byte against a completed observation.

`compare_texture_layout.py --baseline DIR --candidate DIR --output DIR` rejects
incomplete scenes, mismatched commands, capacity differences, unequal exact
color/depth planes or unequal numerical work. It reports line allocations,
128B descriptors, actual valid texture-return bytes, consumed Groups, miss
rate, wait observations and frame cycles. Repeated tag lookups and offered
request cycles are not extra misses. The Group denominator changes with the
layout; texture outputs are the stable secondary denominator. Wait categories
overlap and include intentional service bubbles, so they are not summed.

The whole-frame comparison starts cold after reset and TEX_INVALIDATE, retains
texture lines across all objects and ends with framebuffer FLUSH. It excludes
CPU submission and concurrent Display. It is not board FPS or a warm-redraw
measurement. Rabbit/RGB use their existing texture-bypass materials.

## Verification boundary

The independent RTL tests cover all mip exponents0..10, full/base layouts,
all128 local origins, nonuniform texels and asymmetric weights, six conflicting
lines, CE/commit/output/refill stalls, held Q, all48 FIFO credits, native and
portable RAM, and failures of either child sector. The actual Group/cache
boundary preserves warm hit II2. With a fixed16-beat peer the consecutive-miss
interval is38 clocks versus the baseline's20; this is not physical MC timing.

`fit_texture_layout.py` performs a matched54MHz cache-leaf fit with identical
registered ports, device and placement0. It excludes packet/membership logic,
the complete GPU, CPU, Display and board integration. Its output must not be
used as a whole-chip resource or timing qualification.

## Measured outcome

The complete camera0 native frame passed independent exact numerical and
whole-memory audits in both configurations. Commands, observer TB, MC and frozen
oracle identities match. Color and depth planes match byte-for-byte. The frame
contains96,000 texture outputs; it starts cold and retains lines across all
objects. It does not measure a second warm redraw.

| Metric | Baseline64x128B | Candidate32x256B | Change |
|---|---:|---:|---:|
| Parent-line misses |112|60|-46.43%|
| Consumed Groups |126011|118447|-6.00%|
| Misses/Group |0.088881%|0.050656%|-0.038225 percentage points|
| Accepted128B sectors |112|120|+7.14%|
| Valid returned texture bytes |14336|15360|+7.14%|
| Parent refill wait clocks |4623|5048|+9.19%|
| Head clocks without read issue |130656|123502|-5.48%|
| FIFO-full clocks |4617|3271|-29.15%|
| Complete frame clocks |664381|664766|+0.058%|

The lower miss count is not a bandwidth or speed win. Each candidate miss
fetches twice the data; this scene reads1KiB more and takes385 extra clocks.
Mean parent descriptor occupancy rises from41.277 to84.133 clocks. The
candidate is retained as a correct layout-unification experiment, not selected
as a production performance improvement. No16KiB candidate is mixed into this
comparison.

Matched cache-leaf fit (same registered boundary, device, placement0 and54MHz):

| Metric | Baseline | Candidate |
|---|---:|---:|
| Logic |474|437|
| Registers including179 wrapper FF |298|283|
| CLS |438|390|
| BSRAM |7|7|
| Local Fmax MHz |85.207|81.377|
| Setup/hold violated endpoints |0/0|0/0|

These leaf numbers exclude the changed packet/membership producer and are not
whole-GPU savings. The GPU standard suite passed810 tests with156 ignored;
strict GPU Clippy passed. Targeted native/portable tests retain Group hit II2,
all48 FIFO credits and the registered lookahead. Either child failure prevents
publication; abort before presentation, during a held second request and after
its acceptance drains correctly.

The first long-run receipt attempt was rejected because concurrent Cargo
regression rebuilt the ordinary gold-executable path. Raw completed traces were
preserved; the complete audits were repeated using a content-addressed frozen
oracle after byte-identical DUT/TB/MC/commands/input checks. Original failure
logs remain alongside the audited artifacts. Subsequent harness runs freeze
the oracle before simulation.

No CPU source, RTT view or board selection changed. No whole-board bitstream,
matching RTT firmware, board programming, UART or capture qualification was
performed. Runtime/fit receipts and raw logs remain under the experiment's
isolated target/texture16x8 directory.

The independent [macro-column16 experiment](/ip/gpu-v2/studies/texture-column16/README.md)
retains this cache capacity and local tile format while comparing macro order,
padded framebuffer allocations, FB/Display/CLEAR and full-scene traffic.


Timing-scope correction: the historical full-scene peer is approximately49.5/99MHz
with CL2, as specified by its archived TB. Old immutable receipts that say
normal54/108 describe the wrong frequency; their matched cycle/traffic comparison
remains valid. Production54/108MHz CL3 whole-board qualification is separate.
