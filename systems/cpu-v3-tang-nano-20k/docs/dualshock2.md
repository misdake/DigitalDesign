# DualShock 2 input

The full CPU system binds Device5 to an original single-byte serial engine in
`src/hardware/dualshock2/`. Software in `rcc/dualshock2.rs` owns complete packets,
configuration, validation, buttons, all four axes, event baselines and reconnect.
There is no packet FIFO, DMA, pressure decoder or vibration controller.

## Device5 contract

All accesses use the normal CPU device bus. Unselected devices and unused read
channels return zero. Device0 allocations are unchanged. Device5 uses channels
0 through 3 in `TangNano20kDeviceLayout`.

| Channel | RECV | SEND |
|---|---|---|
| 0 | bit0 busy; bit1 result-valid; bit2 selected (ATT low); bit3 sticky rejected; other bits zero | Exact value 1 selects; 2 deselects; 3 aborts; other values reject |
| 1 | zero | Transfer low eight data bits; upper bits ignored |
| 2 | Shift register in low eight bits | Explicitly consume pending result; data ignored |
| 3 | zero | Clear sticky rejected if bit0 is one; otherwise no effect |

Select is accepted only while idle, deselected, with no pending result. Transfer
is accepted only while idle, selected, with no pending result. Deselect is
accepted only while idle, selected, after result consumption. Consumption is
accepted only while idle with a pending result. Invalid operations set rejected
without modifying the active transfer, existing result, or ATT. Unknown write
channels also reject. Clearing rejected never consumes data or stops work.
Rejected does not itself prevent a later legal operation.

Synchronous active-high reset and abort have priority over progress. They clear
the state, result and rejected, and drive ATT/CLK/CMD high. Abort deliberately
terminates a partial packet and may shorten a low CLK pulse; it is a recovery
operation. Reset also initializes both DAT synchronizer FFs high. Normal
deselect cannot truncate a byte or its guard.

RECV and Device0 watch probes are entirely side-effect-free. Result-valid rises
only after guard completion and stays high until explicit consumption or
reset/abort. Channel2 is meaningful only with result-valid; while busy it can
expose the intermediate shared shift value. Consumption clears valid but need
not erase that value. No SEND in the accepting edge of completion is implicitly
accepted: acceptance uses the old busy/valid values.

## Main-clock serial schedule

All engine state and the two DAT synchronizer stages run on the ungated 54 MHz
main clock. CLK is an output, never an internal clock. CPU hold, cache stalls,
watch sleep, or stopped instruction retirement do not stop the engine.

| Boundary | Action and lifetime |
|---|---|
| Accepted select | ATT low; CLK/CMD high; busy for 216 clocks (4 us setup) |
| Accepted transfer | Load one 8-bit Tx/Rx register; busy; wait 216 clocks |
| Falling CLK | Present old shift bit0 on separate CMD FF; hold CMD through following rise |
| 216 clocks later, rising CLK | Shift right, insert second DAT synchronizer FF into bit7 |
| Next falling CLK | 216 clocks after previous rise; repeat for eight LSB-first bits |
| Eighth rising CLK | Shared register now contains received byte; start guard |
| 32 half-periods later | 6912 clocks = 128 us guard elapsed; busy low, result-valid high, CMD high |
| Accepted deselect | Hold selected, CLK/CMD high for 216 clocks, then ATT high and busy low |

The first falling edge follows transfer acceptance by 216 clocks. Transfer
acceptance to result publication is 48 half-periods = 10368 clocks = 192 us.
Software gaps may lengthen high CLK/ATT setup, never shorten edges or guard.
DAT is sampled two main-clock stages after the pin; an external controller must
settle before the sampling edge with that synchronizer delay allowed.

The shared five-bit counter increments for both phases. Eight sampled bits
advance it from 0 to 8; guard then advances through 31 and wraps to 7 before
publishing the result on the next half-period tick. Both phases test the same
terminal value7, sharing arithmetic without changing the 32-tick guard.

The RTL declares 31 state bits: shift8, divider8, shared bit/guard count5,
phase3, CLK/CMD/ATT3, DAT synchronizer2, result-valid1, rejected1 (31 bits total).
It contains no memory primitive, DSP, general multiply/divide or new clock
domain. Actual mapped logic, routing and physical timing require the complete
board build; declarations and resource reservations are not fitted results.

## CPU protocol and snapshot

`ds2_packet(rx, command, arg)` selects, transfers 01/command/00, and validates
FF/ID/5A before accepting a recognized length. IDs 41, 73, 79 and F3 select
5, 9, 21 and 9 bytes respectively. Unknown IDs never control an unchecked length.
Every recognized packet is completely clocked before normal deselect. F3 denotes
configuration and is never published as input. No 4F pressure or 4D vibration
configuration is sent; a 79 reply's pressure tail is received and ignored.

`ds2_service(state, rx)` performs at most one packet per call. A digital 41 poll
starts 43(entry, arg1), 44(analog+lock 01/03), 43(exit, arg0 with 5A fill), then
polls again. Only a complete 73/79 poll establishes connected input. Startup in
F3 schedules exit and reprobe. Integer wall-time gating uses two millisecond
ticks after configuration packets, guaranteeing at least 1 ms despite timer
quantization. Normal connected polls wait 16 ms after the preceding packet.
The caller must service regularly; long blocking render/wait loops must arrange
their own service opportunities.

Three unsuccessful configuration rounds lead to a 250 ms digital retry delay.
Transport failures retry after 10 ms, then 250 ms after three failures. A later
valid analog packet resets the failure count. There are no blocking retry loops.
Configuration packets span multiple calls, with ATT released between packets.

The caller supplies state[16] and rx[21] in CPU memory:

| State index | Meaning |
|---|---|
| 0,1,2 | Analog connected flag, last packet ID (zero on transport/header failure), active-high buttons |
| 3,4,5,6 | Raw right X/Y, left X/Y, each 0..255 |
| 7,8,9 | Prior buttons, newly pressed, newly released |
| 10,11 | Configuration phase (0 poll, 1 enter, 2 mode, 3 exit), bounded failure count |
| 12,13,14,15 | Last completion ms, delay ms, started flag, reserved |

Buttons decode active-low little-endian packet bytes3/4. Axes are bytes5..8.
No axes are inferred from a digital reply. Disconnect, malformed header or
timeout clears connected/buttons/events and sets raw axes neutral128, whose
validity is explicitly false. Reconnect establishes prior buttons without a
synthetic press. New edges last until the next service call. Consumers requiring
queued events must accumulate them themselves. `ds2_axis(raw, deadzone)` subtracts
128 and removes the inclusive symmetric dead zone; use raw<=255 and
deadzone<=127. Raw values stay available and no startup calibration is performed.

## Timeout and timer ownership

Each packet reads a Device0 channel11 start value. Byte helpers only read this
timer and never clear it. At >=20000 us elapsed, saturation65535, or a detected
backward clear during a packet, software aborts and rejects the packet. This is
an elapsed-time bound, independent of instruction counts. A CPU stopped past the
deadline rejects the result on resumption even if serial transfer completed.
The minimum program owns clear once per service; a future frame scheduler must
provide the shared clear boundary and never clear during a packet. Saturation
cannot represent a valid zero or wrapped elapsed interval.

There is no ACK input and no CRC. Header/known-length validation cannot detect
every bit error. In particular, a controller that stops driving after a valid
header can leave high bits indistinguishable from legal FF payload or ignored
pressure bytes. A missing/short header is rejected; arbitrary short payloads
cannot be guaranteed detectable with this six-wire contract.

## Board ownership and validation boundary

`cold_boot_hdmi_stream_ds2_memory_project` retains the existing 54/108 MHz CL3
memory/CPU/GPU/Display profile and SDC, while replacing the user-I/O binding.
Only LEDs0/1 (pins15/16, active-low) remain. DS2 CLK/CMD/DAT/ATT bind to
17/20/19/18 at LVCMOS33; DAT is input-only with weak pull-up. Existing six-LED
profiles remain available for other targets. The onboard LED physical loads
on these pins remain present and must be measured with actual cabling.

Bounded independent serial-peer tests cover duplex bytes, exact edges/guard,
DAT synchronization, backpressure, illegal starts, reset/abort and CPU pause.
Actual CPU/D-cache/Device0/Device5 tests execute configuration and full polling,
including 79 tail, four-axis boundaries, bad/short headers, unknown ID,
unplug/reconnect, watch, long pause and saturation. They exchange wire bits;
they do not preload decoded button/axis results. Pin/export checks assert unique
CST ownership, preserved SDRAM constraints and unchanged DSP/BSRAM reservations.
These checks do not qualify a routed image or prove electrical compatibility.

The [standalone test](/systems/cpu-v3-tang-nano-20k/examples/dualshock2/README.md)
documents cold-boot packaging, UART fields, wiring and user hardware checks.

The separate [GPU UI diagnostic](/systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/README.md)
displays raw axes, connection/mode, held and accumulated edge masks,16button
indicators and two stick panels. It keeps polling during GPU/present waits,
owns the interval timer clear, and reuses the same Device5 hardware.
