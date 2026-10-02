---
id: gowin-bsram-timing
status: frozen
last-verified: 2026-10-02
---

# GW2AR-18 BSRAM: construction, ports, edge timing

Target: `GW2AR-LV18QN88C8/I7` (Tang Nano 20K), Gowin V1.9.8.11 Education.
Verified: 2026-10-02. Executable evidence stays in `D:/fpga/experiments/gowin-bsram-timing/`.
Scope: primitive geometry + functional edge timing. No new PnR/Fmax or board measurement.

## 1. Physical construction and cost

One block = shared array + two access ports + independent read output stages.
16/18-Kbit modes are alternative geometries of that block. `X9` exposes 9-bit lanes;
the ninth bit can store ordinary data, without automatic parity checking.

| Mode | Primitives | Port A | Port B | Maximum width per port |
| --- | --- | --- | --- | --- |
| Single port | `SP / SPX9` | one shared read/write address | unavailable | 32 / 36 |
| Semi/simple dual port (SDP, 伪双口) | `SDPB / SDPX9B` | write only | read only | 32 / 36 |
| True dual port (TDP, 真双口) | `DPB / DPX9B` | read **or** write | read **or** write | 16 / 18 |
| ROM | `pROM / pROMX9` | read only | unavailable | 32 / 36 |

**TDP = two total accesses, each port reading or writing.** Combinations: R/R, R/W, W/R, W/W (§5).
Both ports share contents. SDP fixes directions but permits wider ports.

| Width | Depth in one block | Logical address placed in `AD[13:0]` | SDP | TDP |
| ---: | ---: | --- | --- | --- |
| 1 | 16384 | `[13:0]` | yes | yes |
| 2 | 8192 | `[13:1]` | yes | yes |
| 4 | 4096 | `[13:2]` | yes | yes |
| 8 / 9 | 2048 | `[13:3]` | yes | yes |
| 16 / 18 | 1024 | `[13:4]` | yes | yes |
| 32 / 36 | 512 | `[13:5]` | yes | **no** |

The 1/2/4/8/16/32 family exposes 16384 bits; 9/18/36 exposes 18432 bits.
Mixed widths are static per-port parameters within one family; do not mix 16 with 18.
Example minimum geometries: `512×32 1R1W` = 1 SDP; `512×32 2RW` = 2 parallel DPB;
`512×36 2RW` = 2 parallel DPX9B; `512×32 2R1W` = 2 mirrored SDP with broadcast writes.
Width/depth banking stores different bits/words; replication stores copies to add read ports.
Extra logical banks inside one array add **no** physical access ports.
Actual inference: 512×32 TDP → 2 DPB (`test_resources/README.md` under the local FPGA workspace);
width stress → template-dependent splitting (`validated_fpga_snippets/projects/06_bsram_width/README.md`).

## 2. Ports and address wiring

Controls are active high; transfers sample on the port's **rising edge**.

| Function | SDP | TDP | Meaning |
| --- | --- | --- | --- |
| Clock | `CLKA / CLKB` | same | A and B may use independent clocks |
| Array access enable | `CEA / CEB` | same | gates the current port transaction |
| Address | `ADA[13:0] / ADB[13:0]` | same | encoded physical address, not a raw word index |
| Write data | `DI[31:0] / DI[35:0]` | `DIA,DIB[15:0] / [17:0]` | use low configured-width bits |
| Write/read choice | **no WRE** | `WREA / WREB` | TDP: 1=write, 0=read; SDP: enabled A always writes |
| Read data | `DO[31:0] / [35:0]` on B | `DOA,DOB[15:0] / [17:0]` | no built-in valid/ready |
| Final output enable | `OCE` on B | `OCEA / OCEB` | gates only the optional pipeline output stage |
| Output reset | `RESETA / RESETB` | same | reset read stages, **not memory contents**; tie SDP RESETA low |
| Block selection | `BLKSELA/B[2:0]` | same | access enabled only when equal to `BLK_SEL_0/1` |

Single block: block-selection parameters and pins = `3'b000`. Parameters:
SDP `BIT_WIDTH_0/1, READ_MODE`; TDP `BIT_WIDTH_0/1, READ_MODE0/1, WRITE_MODE0/1`;
both `RESET_MODE, INIT_RAM_00..3F`.

**Address low bits are also write lane enables.** In this local primitive library:

| Width | Full-word write address | Read address | Enabled lane |
| --- | --- | --- | --- |
| 8 / 9 | `{index[10:0],3'b000}` | same | whole word |
| 16 / 18 | `{index[9:0],2'b00,2'b11}` | `{index[9:0],4'b0000}` | `AD[0]`: low 8/9 bits; `AD[1]`: high 8/9 bits |
| 32 / 36 (SDP only) | `{index[8:0],1'b0,4'b1111}` | `{index[8:0],5'b00000}` | `AD[3:0]`: four 8/9-bit lanes, low to high |

All-zero low bits on a 16/18/32/36-bit write enable **no lanes** in this model.
Verify masked-write mapping in the netlist: the linked byte-write template consumes two SP blocks.

Mixed-width packing: wide word `j` contains narrow words `2j` in low bits and `2j+1` in high bits
for 32↔16 or 36↔18. The logical indices differ; collision checks compare overlapping storage ranges.

## 3. Read timing: two stages, two independent enables

Conceptual read path (one instance per readable port):

```text
AD + array --[read edge, CE & selected]--> BP --[next edge, OCE]--> PL
                                           |                     |
                                     READ_MODE=0            READ_MODE=1
                                           +-------- DO ---------+
```

`BP` = sampled read stage, `PL` = optional output register. At each edge,
compute both from **pre-edge** state, then commit together:

```text
if reset:                   BP' = 0;          PL' = 0
else:
    if enabled read:        BP' = M[address]  else BP' = BP
    if OCE:                 PL' = BP          else PL' = PL
DO = BP (bypass) or PL (pipeline)
```

Enabled read = `CE & selected` on SDP B; add `!WRE` on either TDP port.
TDP writes modify BP according to §4. This is a functional edge model, not gate-level implementation.

Define `E0` as the edge sampling a request already stable before it. `Q(x)` is its stored data.
The table shows **after-edge** outputs; all enables are 1, reset is 0, old output is `-`.

| Edge | Address sampled | Bypass DO after edge | Pipeline DO after edge |
| --- | --- | --- | --- |
| E0 | x | Q(x) | - |
| E1 | y | Q(y) | Q(x) |
| E2 | z | Q(z) | Q(y) |
| E3 | read disabled, OCE=1 | Q(z) | Q(z) |

**Bypass is synchronous**: changing AD between edges does not change DO. Pipeline adds one edge;
both accept one read per active edge. A downstream FF samples Q(x) at E1 / E2 respectively.
An upstream FF launching AD at E0 postpones RAM acceptance to E1: add that launch stage.

Pipeline stalls must account for both stages:

| CE & selected | OCE | BP after edge | PL after edge |
| --- | --- | --- | --- |
| 0 | 0 | hold | hold |
| 0 | 1 | hold | previous BP: an outstanding read can drain |
| 1 | 0 | new read | hold: continued reads overwrite intermediate results |
| 1 | 1 | new read | previous BP |

OCE is ignored in bypass. Block deselection stops access, **not PL**. Whole-pipeline freeze requires
CE=0 and OCE=0; draining requires OCE=1. Track request valid/tag and consumption explicitly:
copying held BP or repeating DO must not create another response. No queue/automatic backpressure.

## 4. Write timing and read-modify-write

Write acceptance: SDP A `CLKA↑ & CEA & selected`; TDP `CLK↑ & CE & selected & WRE`.
Gate SDP CEA with write-valid. AD/DI/mask/enables must be stable before the edge;
the output pipeline adds no storage-write stage.

TDP write modes describe the **writing port's own BP**, not the other port's collision response:

| WRITE_MODE | At a write edge | Bypass output | Pipeline output |
| --- | --- | --- | --- |
| `00` normal | memory updated; BP holds | previous output | previous BP if OCE=1; may drain an earlier read |
| `01` write-through | memory updated; BP becomes updated word | written word (unwritten lanes preserved) | written word at next OCE-enabled edge |
| `10` read-before-write | old local models implement it | **unsupported for GW2A(R) TDP** | do not use |

UG285 1.4E prohibits TDP read-before-write for 55-nm families; the installed older manual/model
still advertise `10`. Follow the current restriction even if simulation passes.

For same-clock, non-colliding write→read: write x at E0; read x at E1; bypass exposes the new word
after E1, pipeline after E2 with OCE=1. SDP uses A for E0 and B for E1; TDP can use either port.
At E0 the other port may read a **different** address, overlapping independent work.

Earliest read-modify-write using combinational logic from DO to write data:
read x at E0 → bypass Q(x) after E0 → write f(Q(x)) at E1;
pipeline Q(x) after E1 → write at E2. Extra consumer FFs, arithmetic stages or stalls add edges.
Reserve the address/ownership throughout; TDP does not make a multi-edge RMW atomic.

## 5. Collision, clocks, reset

| Concurrent accesses to overlapping storage | Contract |
| --- | --- |
| R/R (TDP), including same address | allowed |
| R/W or W/R (SDP or TDP) | **forbidden by current UG285**; arbitrate, bypass externally, or transfer ownership |
| W/W (TDP) | forbid overlap; no defined winner |
| Non-overlapping addresses | parallel access allowed |

Model old-data output during a forbidden collision is not a silicon guarantee; `WRITE_MODE` cannot
repair cross-port collisions. Unrelated clocks need ownership/CDC (FIFO pointers or published banks);
different edge timestamps alone are insufficient. Count read latency on the receiving clock after
ownership arrives; there is no fixed write-clock→read-clock latency.

`RESET_MODE="SYNC"`: stages clear on their clock edge; `"ASYNC"`: clear immediately.
Reset takes priority over CE/OCE. It clears BP and PL even in bypass, preserves the array,
and is **not a write inhibit**: keep reset low during writes and explicitly gate writes during reset.
`INIT_RAM_00..3F` initializes configuration contents: 64×256 bits / 64×288 bits for non-X9 / X9.
It is not runtime reset; software validity must not depend on simulation defaults.

## 6. Evidence and reproduction

- [UG285 1.4E](https://cdn.gowinsemi.com.cn/UG285E.pdf): §§2.2, 3.1, 3.3, 4;
  geometry, port tables, extra pipeline edge, collision prohibition, TDP mode restrictions.
- Installed `IDE/doc/EN/UG285-1.3.3E_Gowin BSRAM & SSRAM User Guide.pdf` under `GOWIN_HOME`:
  figures 3-1/3-2, 3-9/3-10, 4-1 visually checked; current guide overrides old TDP mode claims.
- Local DS226 2.1 (`DS226-2.1_GW2AR系列FPGA产品数据手册.pdf` in the FPGA workspace): §§3.5.4–3.5.7, byte enables,
  ninth-bit storage, synchronous operations, configuration initialization.
- `IDE/simlib/gw2a/prim_sim.v`: `gowin-bsram-timing/rtl/timing_tb.sv` under `FPGA_EXPERIMENTS` passes **32 cases**
  (8 SDP + 16 TDP + 8 mixed-width): both read modes, independent port pulses, both write modes,
  CE/OCE/select, 8/9-bit masks, non-overlapping 2W/2R, both resets, retained contents.
  Limit: 50 µs; no illegal collision golden, new PnR or board claim.

```powershell
& (Join-Path $env:FPGA_EXPERIMENTS 'gowin-bsram-timing/scripts/run.ps1')
```

Set `FPGA_EXPERIMENTS` to the local experiments directory. Requires `GOWIN_HOME`,
`IVERILOG_EXE`, `VVP_EXE` (last two fall back to PATH).
Outputs under the experiment workspace: `build/simulation.txt`, `build/manifest.json`
(library/test SHA256), `build/timing.vvp`.
Integration: audit netlist primitive/count/width/modes/extra FFs and clocks; check whole-design STA.
Functional edge counts do not imply a particular Fmax.
