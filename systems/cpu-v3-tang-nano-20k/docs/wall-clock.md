# System wall clock

`SystemControlDevice<CLOCKS_PER_BIT, CLOCKS_PER_MS=54000>` counts every
main-clock edge independently of CPU hold, cache maintenance, DWAIT and watch
sleep. At 54 MHz its explicit second argument is 54000. The implementation
uses a 16-bit prescaler and a 16-bit millisecond count: 32 register bits before
synthesis, constant terminal comparison and two incrementers. It adds no RAM,
DSP, general divider, multiplier or clock domain. Physical logic and timing
costs have not been measured.

## Device interface and edges

Device 0, read channel 10 (`SYSCTL_MILLISECONDS`) returns the current 16-bit
millisecond count. Reads have no side effects; writes are ignored. Other
device indices see zero from SystemControl. Channels 8 and 9 are unassigned
by this interface.

Synchronous active-high reset clears the count and prescaler. The first
increment occurs on the 54000th non-reset rising edge. A same-domain RECV
samples the count before that edge's increment. The Rust hardware emulator
implements the same edge and reset behavior as generated RTL.

The positive u16 `CLOCKS_PER_MS` parameter is independent of UART baud-rate
rounding. The complete 54 MHz system explicitly instantiates `<469,54000>`;
a 27 MHz instance uses `CLOCKS_PER_MS=27000`. Legacy one-parameter instances
default to the 54 MHz millisecond divisor.

The count wraps every 65536 ms. Unsigned 16-bit subtraction gives the elapsed
interval only when its actual duration is strictly less than 65536 ms. Two
timestamps cannot detect a whole missed wrap. Integer-millisecond quantization
contributes less than one millisecond of interval error.

## Software and functional simulation

RCC programs include `device_abi` and `time`, then call `time_milliseconds()`
and `time_elapsed_ms(start, end)`. The latter performs unsigned 16-bit modular
subtraction; it does not detect missed full wraps or validate a deadline.

The architecture-only `boot::SystemControlDevice` exposes
`advance_milliseconds(elapsed)` to its host through `CpuV3Sim::device()`. This
functional model has no physical clock: the host supplies elapsed time,
including time spent waiting. Reads, writes and retired instructions never
advance time. Default construction starts at zero; constructing a new model
resets it. This host model is not cycle-accurate performance evidence.

## Verification boundary

The production SystemControl tests check the exact 54 MHz divisor, a smaller
divisor, natural wrap, reset, read-only behavior and counting during maintenance
and watch holds. A real CPU/cache/SystemControl test checks RECV sampling and
retirement, time advancing during LCOPY/DWAIT, cache clean and watch sleep, and
the cleaned SDRAM word. It uses an independent delayed, backpressured memory
peer. These checks do not establish full-system fit or physical board timing.

The [frame statistics example](/systems/cpu-v3-tang-nano-20k/examples/frame_stats/docs/components.md)
defines how completed frame intervals become character IDs; it does not own
the system clock or its device interface.
