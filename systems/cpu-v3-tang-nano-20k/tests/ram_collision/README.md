# RAM collision read consumption checks

These bounded offline checks use the real CPU/FPU, D-cache and Display RTL with
Gowin GW2A RAM models. Only the test copies of RAM read returns are changed:
same-address cross-port read/write returns become unknown, while writes, read
enables and latency retain their contract. Valid operands, commits, CPU loads,
writebacks and pixels have independent oracles. No production RTL is patched.

## Run

Set `IVERILOG_EXE`, `VVP_EXE` and `GOWIN_HOME`, with Python 3.10+ and the pinned
Rust toolchain available. From the repository root, regenerate inputs whenever
production sources change, then run serially:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label ram-collision-export -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','ram_collision_consumption',
  '--','--ignored','--nocapture','--test-threads=1')
python -B systems/cpu-v3-tang-nano-20k/tests/ram_collision/run.py
```

`--case fpu|dcache|display` selects one case. The exporter only writes RTL; its
successful test result alone is not a simulation pass. The Python runner runs
baseline, collision poison and a live-Q corruption detector for each case.
Both positive variants must pass; every detector must fail at its named oracle.
Outputs stay under `target/ram-collision-consumption`. Each case records exact
commands, input SHA256s and logs; the manifest records export Git identity,
fixture/peer/RTL hashes, primitive-library identity, bounds and results. No old
review checkout, fitted netlist or previously generated target is required.

## Coverage and limits

| Case | Directed workload | Required positive coverage | Cycle cap |
| --- | --- | --- | --- |
| FPU | Actual CPU FLD4, aliased VADD/VMUL, scalar dependencies, broadcast VMULS overwriting Fb, VMOV, FST4 | 27 ordered writes; collisions hit; zero tainted effective operands; known stored halfwords | 12000 core cycles + 12500 backup |
| D-cache | Even/odd way1 stores, immediate same/different-address loads, dirty eviction/refill and clean | 2 way1 stores; collisions hit; at least 8 checked writeback beats; all 4096 backing halfwords checked | 20000 clocks |
| Display | Continuous scan/blanking, software present, delayed first legal descriptor, two-row ownership and both clocks | At least 2 exact bank collisions; zero tainted effective consumers; 32000 independently checked valid pixels, black and blank coverage | 2217600 pixel clocks |

Each compile has a 60-second wall watchdog. FPU/cache runs have 60 seconds;
Display runs have 90 seconds. CPU response and all cache write-data beat
positions encounter legal stalls; the admitted physical read stream is not
backpressured. Display keeps 18.518ns logic and 30.030ns pixel periods. Its fixed
12.526ns pixel phase aligns the first legal same-address write/read at
33300354601ps, one event in each bank. Missing collision coverage fails rather
than being called an exclusion. The wider 30.031ns poison window also covers
writes while read Q is held; it is not a measured silicon collision aperture.

`reference.json` records the audited ba4803cf fit's RAM modes, initialization
digests and primitive-library hash. `models.py` regenerates initialization from
exported production assignments, uses the RAM default zero for unspecified
words and checks it against those digests. It stores no second initialization
array. FPU and Display use 512x32 SDPB bypass reads; cache data and metadata use
16-bit DPB bypass reads/normal writes. D-cache reuses its tracked legal test peer.

The directed ba4803cf runs found 6 FPU, 2 D-cache and 2 exact Display bank
collision events, with none entering valid consumption. The three detector
faults were rejected. These cases add CPU/vector/memory dependencies beyond the
old scalar FPU fixture; they do not fix or explain board flicker. They do not
qualify physical CDC/metastability, PVT, serial TMDS (the existing Icarus bypass
remains), a complete real-content frame, every asynchronous phase, special FPU
LUT consumers, or all maintenance/swap/error paths.

## Files

- `run.py`: bounded replay, source preparation and evidence manifest.
- `fpu_program.py`, `fpu_tb.v`: generated legal program and independent CPU/FPU oracles.
- `dcache_checks.vh`: directed checks appended to the existing tracked cache peer.
- `display_tb.v`: fixed dual-clock scanout and independent absolute pixel oracle.
- `models.py`, `rf_checks.vh`, `display_ram_checks.vh`, `reference.json`: test-only native RAM bindings, collision tracking and reference identity.
