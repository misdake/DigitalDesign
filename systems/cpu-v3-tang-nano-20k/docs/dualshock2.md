# DualShock 2 input

The full CPU system binds Device5 to an autonomous packet engine in
`src/hardware/dualshock2/`. Software in `rcc/dualshock2.rs` starts and collects
packets without waiting for serial progress, and owns configuration, complete
ID/header validation, buttons, all four axes, event baselines and reconnect.
There is one in-flight or unread packet, no queue, DMA, pressure interpretation
or vibration configuration. The old single-byte Device5 ABI is replaced; old
applications and byte-engine bitstreams must not be mixed with this driver.

## Device5 contract

All accesses use the ordinary CPU device bus. Device0 allocations are unchanged.
Unselected devices and unused read channels return zero. Device5 owns channels0/1.

| Channel | RECV | SEND |
|---|---|---|
| 0 | bit0 busy; bit1 result-valid; bit2 ATT selected; bit3 sticky rejected; bit4 first received byte was FF; other bits zero | Exact1 consumes one result word; exact2 clears rejected; exact3 aborts; other values reject |
| 1 | Current packed result word: earlier wire byte in high8, later byte in low8 | Start: bits1:0 select0 poll42,1 config43,2 analog44;3 rejects. Bit8 supplies fourth-byte argument0/1; other bits ignored |

Start is accepted only while idle without an unread result. It selects ATT,
clears header-good and starts the complete transaction. The engine sends
01/command/00/argument, followed by zero, except analog44 sends03 at byte4 and
config43 with argument0 sends5A from byte4 onwards. CPU software selects the
configuration sequence; hardware does not run a reconnect or initialization loop.

A read never advances or acknowledges data, including Device0 watch probes.
Consume is accepted only while idle with result-valid. Each consume advances
one16-bit word; consuming the final word clears result-valid. Another start
cannot overwrite an unread packet. Busy starts/consumes and unsupported writes
set rejected without modifying traffic or unread results. Clearing rejected
neither stops traffic nor consumes data. Rejected does not prohibit a later legal
operation. Acceptance on a completion edge uses the old busy/valid values.

Reset and abort have priority. Both clear progress, valid, rejected and header
state, and drive CLK/CMD/ATT high. Abort can truncate a low CLK pulse and is only
for recovery. Reset initializes both DAT synchronizer FFs high. Packet memory
is deliberately not reset; only valid words may be read. Channel1 while busy,
after final consume, or before publication has no specified value. Header-good
remains until the next start/reset/abort and alone does not imply valid input.

## Storage and lifetime

Bytes1..20 occupy ten packed words at addresses1..10 in a16x16 asynchronous-read,
synchronous-write distributed RAM. Gowin maps it to four RAM16S4 cells,256bits;
there is no BSRAM or DSP. Byte0 is represented by header-good, rather than a
second raw register. On success software reconstructs its known valueFF.

The eight-bit serial shift register is shared for transmit/receive. Odd byte
completion captures a high-byte FF; even byte completion writes the pair into
RAM at `byte_index[4:1]`. The first byte also writes unused address0; that value
is never consumed. No dynamic lane mask or array read mux is built in FF logic.
On packet completion the same byte index becomes the CPU cursor2,4,...,20.
The two phases never access RAM concurrently, so no arbitration, copy, extra
read pointer or packet alignment shifter is needed. The final wire word address
also determines when consumption releases the packet.

| Boundary | Stored state and ownership |
|---|---|
| Accepted start | Serial engine owns packet RAM; busy rises, old bytes are invalid |
| Byte0 completed | Record exactFF predicate |
| Odd/even payload bytes | First byte held in pair FF; second completes one RAM word |
| Guard and ATT release completed | CPU owns all result words; cursor points to ID/5A |
| RECV1 | Word remains stable; repeated reads have no side effects |
| SEND0=1 | Advance cursor; final word releases valid for the next start |
| Reset/abort | Invalidate snapshot and release lines; RAM contents are irrelevant |

## Main-clock serial schedule

All state and both DAT synchronizer stages use the ungated54MHz main clock.
CLK is an output, never an internal clock. CPU sleep, watch, cache stalls or
stopped retirement do not stop the packet.

| Boundary | Action |
|---|---|
| Accepted start | ATT low;216clocks setup, then216clocks before first falling CLK |
| Falling CLK | Present old shift bit0 on CMD and hold through following rise |
|216clocks later, rising CLK | Sample second DAT FF and shift right; eight bits LSB-first |
| Eighth rise | Capture received byte and begin exactly32 half-period ticks of guard |
| Guard completed | Load next transmit byte, or start final216clock ATT release |
| Next falling CLK |216clocks after guard;7128clocks after preceding byte's last rise |
| Final release | ATT high, busy low and result-valid high together |

Half-period216clocks is4us:125kHz SCLK,64us wire time per byte,128us guard.
A packet takes `N*10368+432` main clocks:93744 for9bytes and218160 for21bytes.
CPU start/collection instructions are separate. DAT must settle before the
sampling edge with the two-clock synchronizer delay allowed.

The existing five-bit counter is shared by bits0..7 and the guard, incrementing
from8 through wrap to7 for exactly32ticks. The byte index increments by one on
wire progress and by two on CPU word consume. It and the final word index are
shared across serial and CPU ownership, rather than maintained twice.

The reply ID's low nibble selects bounded lengths:1 gives5bytes,3 gives9,
9 gives21, everything else gives3. This is framing only: the CPU requires exact
FF/known ID/5A. An unknown ID sharing a recognized nibble is still rejected.
A malformed packet cannot request an unbounded hardware transfer. Every selected
bounded length is completely clocked before normal ATT release, including the
full79 pressure tail.

## Nonblocking CPU service and snapshot

`ds2_service(state, rx)` starts a transaction and returns false immediately,
checks an in-flight transaction and returns false immediately, or copies a
completed result in one bounded CPU call. It has no wire-wait or retry loop.
It reads ID/5A first, validates header-good and the exact known ID, then reads and
consumes2/4/10 words for5/9/21byte packets. `rx[0..length]` contains the complete
recognized reply; trailing array capacity is unspecified. A failed packet is
never published as input. Only complete73/79 polls return true.

A digital41 poll schedules43(entry,arg1),44(analog+lock01/03),43(exit,arg0,
5A fill), then reprobes. F3 means configuration and is never published as input;
startup in F3 schedules exit first. Each configuration stage completes in a
separate call and waits two integer milliseconds after completion before the
next start, guaranteeing at least1ms despite timestamp quantization.

Connected polling uses a16ms **start-to-start** cadence. A completed poll
collected at/after its next deadline immediately launches the next poll in that
same call. Thus a once-per-frame caller can collect the preceding packet and
start the following one without halving the input rate. Earlier collection
leaves the next start for a later eligible call. Applications must service often
enough to meet the timeout below; no CPU callback is required between bytes.

Three unsuccessful configuration rounds use a250ms digital retry delay.
Transport/header failures retry after10ms, then250ms after three failures. A
valid analog reply resets the failure count. There is no blocking retry loop.

The caller supplies state[16] and rx[21] in CPU memory:

| State index | Meaning |
|---|---|
|0,1,2|Analog connected, last packet ID (zero on failure), active-high buttons|
|3,4,5,6|Raw rightX/Y,leftX/Y, each0..255|
|7,8,9|Previous buttons, newly pressed, newly released|
|10,11|Configuration phase0/1/2/3, bounded failure count|
|12,13|Current packet start or last configuration/failure completion ms; delay ms|
|14,15|Cadence initialized; packet in-flight|

Buttons decode active-low little-endian bytes3/4; axes are bytes5..8. No analog
axes are inferred from41. Invalid input clears connected/buttons/events and sets
axes neutral128; connection validity remains explicit. Reconnect establishes a
new button baseline without synthetic presses. Edges last until the next
service call; consumers accumulate them if needed. `ds2_axis` remains a simple
legacy center/dead-zone helper; raw bytes and the separate stick helper remain
available without startup calibration.

## Timeout and validity limits

An in-flight packet is rejected if service resumes20ms or more after start, or
if rejected is set. Timeout uses the independent modulo65536 millisecond clock;
it neither reads nor clears the shared microsecond interval. Applications may
clear their frame timer while serial work is in flight. Intervals must remain
below the millisecond counter's wrap period. A CPU stopped beyond the deadline
rejects even a completed snapshot on resumption. The hardware transaction itself
is bounded to at most21bytes and cannot depend on software to release ATT.

There is no ACK input or CRC. Known header/length checks cannot detect all data
errors: a controller that stops driving after a valid header can produce high
payload bits indistinguishable from legitimateFF data. Full pressure bytes are
retained but not interpreted. No4F pressure or4D vibration configuration is sent.

## Board and verification boundary

The board remains54/108MHz CL3. Left CLK/CMD/DAT/ATT use17/20/19/18 at LVCMOS33;
DAT is input-only with pull-up and two preserved main-clock synchronization FFs.
Only LEDs0/1 at15/16 remain. No new clock or pin is added. LED loading and
controller wiring still require physical measurement.

Bounded independent serial peers cover complete packets/configuration/pressure,
exact edges and guard, CPU absence, stable readback, illegal overwrites, explicit
consume and reset/abort phases. Actual CPU/cache/Device0/Device5 tests cover
configuration, reconnect, raw axes, edges, malformed packets, watch and long
pauses. The control-peer UI test exercises one frame and once-per-frame polling;
it does not simulate pixels or wire timing. Local synthesis does not qualify
complete-board routing, timing or electrical compatibility.

The [minimal UART test](/systems/cpu-v3-tang-nano-20k/examples/dualshock2/README.md)
and [GPU UI diagnostic](/systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/README.md)
use this driver. The [stick helper](input-sticks.md) owns calibration, square-ray
mapping, dead zones, direction, strength and optional activation hysteresis.
