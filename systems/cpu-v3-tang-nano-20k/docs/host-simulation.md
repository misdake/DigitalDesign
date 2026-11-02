# Host time and controller input

`CpuV3SystemSim` combines functional CPU/GPU results with explicit host events.
It installs Device0, Display3, GPU4 and the latest-only input Device5. CPU steps,
device reads, watch probes, state queries and vblank do not advance Device0 time.
This simulator supplies software results, not RTL cycles or cache timing.

```mermaid
flowchart LR
  T[Injected Duration or monotonic Instant delta] --> H[HostIo event pump]
  X[Selected Windows XInput slot] --> H
  H -->|elapsed| C[Device0 wall time]
  H -->|complete snapshot or disconnect| I[Device5 latest input]
  C --> R[Existing RCC time / watch]
  I --> P[Existing RCC capture / edges / calibrated sticks]
  V[Explicit vblank event] --> D[Display present]
```

Arrows are host calls and software reads. A complete input replaces the latest
sample atomically at its publication call; neither a read nor begin-read starts
acquisition. Vblank remains a separate Display publication boundary.

## Explicit time

`advance_host_time(Duration)` carries fractional microseconds between calls,
advances the wrapping 16-bit millisecond timestamp and saturates the microsecond
interval at 65535. Long durations are reduced by the millisecond counter period
without a large loop or a narrowing overflow. Device0 SEND11 bit0 still clears
the interval alone. Time is quantized to microseconds; this is not a main-clock
prescaler model or an instruction-to-time conversion.

`HostIo::advance` accepts a deterministic elapsed event and focus state.
`RealtimeHostIo::pump` obtains elapsed time from `Instant`, including time while
the CPU is waiting. Resetting a debugger reconstructs its devices and rebases
the host clock. The debugger's legacy JSON `cycles` field contains functional
step calls; its UI labels them steps, not measured hardware cycles.

The debugger enables the real-time pump at startup. CPU execution services it
while running, including bounded library stepping. The independent
`POST /api/host?focused=0|1` heartbeat services elapsed/input events while paused
or waiting, without executing CPU work. Browser focus/visibility events report
loss and regain. State/framebuffer GETs remain observations. Host events are
serialized with debugger commands; focus loss takes effect when its event is
serviced. Vblank-only rAF scheduling cannot drive this pump.

## Latest input and Windows source

`publish_input(Some(ControllerSnapshot))` atomically replaces eight bytes, sets
valid and sticky changed, even for an identical publication. `None` invalidates
a valid sample and sets changed. Repeated disconnected observations do not
invent new publications. Payload reads select the current latest sample; SEND
does not latch a private old copy. A publication during a multi-read attempt
leaves changed set, so the existing bounded RCC reader discards that attempt
and reads the latest sample on its next call. Multiple publications cannot
cancel changed. RCC retains ownership of button edges and digital neutral axes.

The functional model retains Device5's ordinary channel layout and high-SEND
begin-read aliases. Channels 9..12 return zero as explicitly unmodeled background
and wire diagnostics. They provide no serial slot, phase, DAT, ATT or failed-wire
evidence. No serial polling, pressure tail, vibration or pin state is simulated.

XInput is opt-in with `--xinput-slot 0..3`; it is not queried by default.
Windows uses the official `XInputGetState` binding. Other platforms can build
and use deterministic time/snapshot injection, and reject the XInput option
explicitly. A connected focused source is sampled on host events at intervals
of at least 16 ms; disconnected slots are retried after 1 s. Focus loss invalidates
input; regain retries immediately. No automatic switch to another slot occurs.

| XInput | Existing input meaning |
| --- | --- |
| Back / Start | Select / Start |
| Left/right thumb click | L3 / R3 |
| D-pad | Same four directions |
| Left/right shoulder | L1 / R1 |
| Left/right trigger greater than 30 | L2 / R2 button bits independently |
| A / B / X / Y | Cross / Circle / Square / Triangle |

The mapped semantic button mask is inverted into active-low bytes. Reserved
XInput button bits are ignored. The software sample uses analog ID `0x73` and
marker `0x5a`; these identify the existing format, not a serial reply.

### Circular source to shared DS2 geometry

The adapter expands circular stick travel into the square domain expected by the
existing RCC `stick_read`; UI, gamepad and demo camera keep that same DS2 code.
Application source and Device5 ABI remain common to physical DS2 and XInput.
For `u=(X,-Y)/32768` in RCC's right/down convention, let `r=length(u)` and
`m=max(abs(u.x),abs(u.y))`. The adapter applies:

```text
v = 0                         when m=0
v = u * min(r,1) / m          otherwise
```

The shared forward square-ray geometry maps `v` back to the original direction
and radius `min(r,1)`, before application dead zones. Out-of-circle source points
saturate radius at 1 without changing direction. This is a geometry inverse, not
an inverse of dead-zone suppression or outer saturation.

Expansion precedes the original signed 16-bit-to-byte offset/truncation. X increases
right; Y is inverted before quantization so up produces a smaller raw Y. Zero
maps to 128, negative and positive endpoints remain 0 and 255, and every cardinal
axis value retains its original byte conversion. Endpoint arithmetic widens
before negating `i16::MIN`. For example, `(23170,23170)` becomes DS2 bytes
`(255,0)`; default RCC processing gives approximately `(0.707,-0.707)`.

The default per-axis calibration is 0/128/255. Byte quantization, asymmetric
positive/negative ranges and native FPU arithmetic prevent a lossless integer
round trip. The adapter does not inspect or undo application center/min/max
changes. Application inner/outer thresholds remain authoritative: with default
`inner=24/256` and `outer=1`, the final strength remains
`clamp((min(r,1)-24/256)/(1-24/256),0,1)`, subject to quantization. Suppressed
positions are not expanded back out of the dead zone; small source jitter stays
subject to it. Physical DS2 input, snapshot publication and button edges are
unchanged. See Microsoft's [field contract](https://learn.microsoft.com/en-us/windows/win32/api/xinput/ns-xinput-xinput_gamepad).

## Display runner

`cpu-v3-display-sim` defaults to deterministic host events:
`--host-tick-us 16667` advances that duration at each explicit display sample.
`--frames` and `--max-cpu-steps` bound the host schedule and CPU work separately;
the work budget is not converted into physical cycles. `--realtime`, `--window`
or an explicit XInput slot selects real elapsed time, serviced during CPU work
as well as at display samples. A window reports its actual focus; explicitly
enabled headless input has no window-focus gate. Display samples are explicit
events, not a claim of fitted HDMI refresh timing. A halt stops further CPU step
calls while the bounded host display loop can show the final image.


## Native-edge CPU composition

`system_emu::CpuV3SystemEmu` runs the existing core/FPU, fetch, read-only I-cache,
D-cache, memory arbiter and hardware SystemControl emulators against the fitted
Gowin native stream/gear/controller/pin models. Construction consumes one 4M-word
physical `Vec<u16>` preloaded for reset PC/CSEG0. Both cache clients and physical
pin commits borrow that same backing; there is no architectural CPU executor,
private DRAM copy, implicit clean or synthetic memory-delay mode.

```mermaid
flowchart LR
  C[Existing CPU and FPU] --> F[Fetch and read-only I-cache]
  C --> D[Write-back D-cache]
  C --> S[Hardware SystemControl]
  S -->|Retained maintenance and CPU hold| D
  F --> A[Existing arbiter]
  D --> A
  A --> P[Native stream port and edge models]
  P <-->|Physical reads and writes| M[Only backing image]
```

`step` settles the combinational interfaces with a bounded convergence check,
then commits all logic owners once and advances two native clocks. Initial
reset occupies two logic cycles; native initialization and valid-array sweep
hold the core while memory and device clocks continue. Device0 timers count
logic edges. Maintenance follows the actual core drain, retained command,
acceptance and done/error handshake. `physical_memory` observes DRAM directly,
including stale words while newer CPU data remains dirty in D-cache.
I-cache invalidate, core halt and core fault also flush fetch/BTC, using the
same frontend predicate as the fitted top. Already accepted lower responses
retain their provenance and drain without reaching the new instruction stream.

This entry runs preloaded CPU programs only. External device access and watch
probes return an explicit unsupported-device error. It does not install a Flash
application, execute GPU/Display, provide host controller input or supply a
production application runner. Errors terminate the current run. `cycles` and
`native_cycles` count modeled edges; host elapsed time measures only this
implementation, not a fitted-board measurement.

The bounded `native_system_emu` test executes scalar FLD/ADD/FST and dependent
integer load through the real caches. Without a clean, the CPU sees 3.0 while
physical destination words retain their seed. An adjacent Device0 clean must
drain FST halves, execute once, finish through native memory and expose the
correct physical words before CPU halt. No post-halt cleanup is added.
