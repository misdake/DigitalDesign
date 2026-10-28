# Burst facade and shared memory models

## GPU byte-addressed facade

`memory::ports::MemoryPort` supports one outstanding naturally aligned 128-byte
read/write in byte-address units. It reports request acceptance, write-beat
acceptance, ordered 64-bit read delivery and a separate terminal success/error.
There are 16 data beats per full burst. Error may terminate early; earlier writes
remain. Owner lifetime ends at the terminal response, not the last write beat.

Reads reserve all destination credits before admission. A write has its first
beat before presentation and retains the remaining source. Descriptor/data stay
stable while blocked. Physical controllers cannot tolerate arbitrary mid-burst
write-valid holes. `Result::Err` reports caller/adapter protocol error;
`complete=Some(false)` reports memory-terminal failure. CE does not gate owned
traffic. Reset is coordinated externally after ownership drain.

Four separate requests form a 512-byte tile plane; this facade does not silently
enable controller grouping. The selected hardware's
[three master pins](/ip/gpu-v2/docs/memory.md) have their own physical acceptance boundary.

## Adapters and clock owner

The vendor crate owns arbiter, adapter, bridge, controller and SDRAM pin model.
GPU tests/model adapters use a dev dependency. The test burst adapter converts
bytes to halfwords, preserves held state/terminal ownership, and adds no second
burst payload buffer.

The functional frontend MemoryPort reads real payloads. Texture RefillPort
projects submit/step/events from the same service. RefillBridge polls rather
than independently advancing the controller. A composition has exactly one
wall-clock owner; shared service advances once per wall edge, not once per
client.

## Explicit service options

`ports::Service` accepts byte-addressed32/64/128/512-byte jobs and emits
Started/ReadBeat/Complete events. Host queue acceptance differs from physical
arbiter grant. `sim::service::Memory` executes the current arbiter and logical
stream endpoint reference over abstract memory; its counts are not native-clock
or physical performance evidence. A 512-byte job becomes independent naturally
aligned hardware transactions. Texture refills remain128 bytes.

`sim::oracle::Memory` is an independent byte-data reference with explicit
synthetic read/write delays and beat intervals. It contains no SDRAM command,
bank, refresh, legacy calendar or calibrated average. Real transport timing and
seven-client contention belong to the fresh exported stream RTL/pin fixtures.
Both services require finite cycle/request bounds.

Implementation anchors: [GPU port](/ip/gpu-v2/src/memory/ports.rs),
[texture facade](/ip/gpu-v2/src/texture/ports.rs),
[test adapter](/ip/gpu-v2/tests/support/sdram/burst.rs),
[vendor interfaces](/hardware/vendor/gowin/doc/README.md).
