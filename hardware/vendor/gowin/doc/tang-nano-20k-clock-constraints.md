# Tang Nano 20K related clocks and timing allowance

The current stream target uses 27 MHz input, 108 MHz controller, 54 MHz logic
and a 108 MHz forwarded SDRAM clock at 157.5 degrees. The PLL source is
`src/targets/tang_nano_20k/sdram/pll_108m_54m.v`; generated SDC is owned by
`TangNano20K::cold_boot_hdmi_stream_memory_project`. The
[memory-controller contract](/hardware/vendor/gowin/doc/sdram-memory-controller.md)
defines the current pad delays, IODELAY and related-clock transport.

## PLL parameters and generated clocks must agree

For the existing internal-feedback rPLL topology, IDIV/FBDIV static fields encode
divider minus one. CLKOUTD divides CLKOUT by two; ODIV determines VCO frequency.

| Quantity | Current production | Optional 49.5/99 MHz example |
| --- | --- | --- |
| `FCLKIN` | `"27"` | `"27"` |
| `IDIV_SEL` / actual input divider | 0 / 1 | 2 / 3 |
| `FBDIV_SEL` / actual feedback divider | 3 / 4 | 10 / 11 |
| `ODIV_SEL`, `DYN_SDIV_SEL` | 8, 2 | 8, 2 |
| PFD | 27 MHz | 9 MHz |
| VCO | 864 MHz | 792 MHz |
| CLKOUT / CLKOUTD | 108 / 54 MHz | 99 / 49.5 MHz |
| CLKOUTP / `PSDA_SEL` | 108 MHz / `"0111"` | 99 MHz / `"0111"` |
| Forwarded phase | 157.5 degrees | 157.5 degrees |

The example computes CLKOUT = 27 * 11 / 3 = 99 MHz and VCO = 99 * 8 = 792 MHz.
Its phase offset is 4.419192 ns. It is an alternative setting for this topology,
not a hardware-frequency limit or a production selection. A historical module
name containing `108M54M` is not evidence of actual frequency: inspect the PLL
parameters and generated clock periods/waveforms. Check device/tool PLL limits
for the selected part before using another topology or frequency.

For that example, the corresponding SDC is:

```tcl
create_clock -name clk -period 37.037037 [get_ports {clk}]
create_generated_clock -name controller_clk -source [get_ports {clk}] -multiply_by 11 -divide_by 3 [get_pins {u_sdram_pll/rpll_inst/CLKOUT}]
create_generated_clock -name cpu_clk -source [get_pins {u_sdram_pll/rpll_inst/CLKOUT}] -divide_by 2 [get_pins {u_sdram_pll/rpll_inst/CLKOUTD}]
create_generated_clock -name sdram_clk -source [get_ports {clk}] -multiply_by 11 -divide_by 3 -phase 157.5 [get_pins {u_sdram_pll/rpll_inst/CLKOUTP}]
```

Changing only constraints cannot change device frequency. Change PLL parameters
and SDC together when deliberately selecting another clock. Independently retain
and recheck external I/O delays, multicycles and the asynchronous pixel-clock
group. Recheck the forwarded clock's source-latency envelope against each final
routed SDF; PLL phase alone excludes the clock-port route and output buffer.

Fixed-cycle timebases also change. Keeping 469 logic cycles per UART bit at
49.5 MHz gives about 105544 baud; 54000 logic cycles take 1.090909 ms. Clock-derived
SPI, initialization, refresh and controller pacing must be recalculated rather
than copying historical SPI rates. Cycle durations stretch by 12/11 relative to
54/108 MHz. A nominal software clock label cannot establish actual deadlines.

## Explicit setup and hold uncertainty are total allowances

The stream binding keeps the physical PLL and generated clocks at 54/108 MHz,
with setup guards equivalent to full-cycle budgets at 55/110 MHz. Its explicit
totals include the existing 35 ps allowance:

| Capture clock | Setup uncertainty | Hold uncertainty |
| --- | ---: | ---: |
| `cpu_clk` | 0.371700 ns | 0.035 ns |
| `controller_clk` | 0.203350 ns | 0.035 ns |

The source collection is `{controller_clk cpu_clk sdram_clk}`. The setup guard
is the physical-period minus target-period difference, plus 0.035 ns; it also
applies conservatively to cross-clock paths ending in that capture domain.
SDRAM-referenced output setup, external IO delays, source latency and independent
pixel clocks retain their existing constraints. This is a PnR engineering
budget, not a PLL frequency change or a guarantee of board stability.

A retained Gowin V1.9.11.03 probe reported automatic `tUnc` magnitudes of
0.035 ns on its related-clock paths. With explicit values, setup changed from
`-0.035` to `-0.200` ns and hold from `+0.035` to `+0.100` ns. In that tested
configuration, explicit values replaced the automatic contributions: 0.200 and
0.100 ns are totals, not an extra 0.200/0.100 ns added to 35 ps. Paths without an
automatic contribution receive the entire explicit allowance.

```tcl
set_clock_uncertainty 0.200 -setup -from [get_clocks {controller_clk cpu_clk sdram_clk}] -to [get_clocks {controller_clk cpu_clk sdram_clk}]
set_clock_uncertainty 0.100 -hold -from [get_clocks {controller_clk cpu_clk sdram_clk}] -to [get_clocks {controller_clk cpu_clk sdram_clk}]
```

These example engineering allowances are not selected by current production SDC
and are not official jitter or PVT bounds. The collections include same-clock,
cross-clock, half-cycle and SDRAM-referenced I/O paths without restricting edge
sense. Keep independent pixel-clock constraints. After a tool-version or
constraint-precedence change, repeat a bounded source/SDC probe and inspect real
setup and hold `tUnc` rows; command spelling alone does not prove additive or
replacement semantics.

Setup slack cannot establish hold safety. Check complete routed setup and hold
reports, including falling edges, SDRAM data and output-enable paths. Increasing
setup uncertainty does not repair hold violations. Clean STA and a lower clock
alone do not establish board stability or identify a fault's unique cause.

## Observe the input actually captured

The fitted DQ path is `IOBUF.O -> IODELAY.DI -> DO -> capture.D`. The pin fixture's
read setup/hold and unknown-value checks therefore observe the controller's
`dq_sample`, after IODELAY. Its command/DQM and write-DQ checks still observe pad
signals. A raw-pad transition is not the consumed-input transition; raw poison
may be filtered or shifted while a delayed transition can violate the capture
window even after the pad has stabilized.

The five bounded `cl3_input_monitor.py` probes use the existing portable 32-tap
model: raw poison and a raw near-edge transition must not falsely fail a stable
consumed input; consumed setup, hold and unknown-value faults must fail. The
separate real-controller stream replay retains independent data, LAST, client
contention, refresh and reinitialization checks. Neither a portable delay model
nor monitor probes replace routed SDF, device/PVT or board qualification.
