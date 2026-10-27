# Opaque record ownership

## Objects and capacity

`geometry::record_transport` is a Rust ownership/storage controller for opaque
36-bit rows. It executes no setup, raster or interpolation arithmetic.
`geometry::source_record_link` connects a normal-path snapshot lease from
SourceCapture to this controller.

Exactly two slots each hold 1..64 sequential rows. One writer can fill a Writing
slot while the shared coverage/attribute reader uses a different Published
slot. Payload arrays are not cleared on allocation; validity and generation
establish ownership. Keys bind slot, generation, source ticket, immutable context
and fan index. A source snapshot admits up to six fans. The third fan waits if
both slots are owned; there is no hidden fan FIFO.

| Boundary | Meaning |
| --- | --- |
| source_captured | Controller accepts an already-owned complete snapshot ticket/context |
| SnapshotLastUseAck | Every announced fan is fully written/published, or fan count is zero; snapshot may be consumed |
| RecordReleased | Last marked attribute return transfers and last_quad_ack arrives; record credit returns |

The source link holds one offered ticket through SnapshotAccepted, then returns
last-use feedback on a later enabled edge. Raw source-slot release, snapshot
consumption and numerical record release are distinct ownership events.

## Write, publish and read

Eligibility uses pre-edge state. A final write is followed by a separate enabled
publication edge. Newly published records cannot be read on that edge, and a
release cannot finance same-edge allocation.

One shared return credit covers Pending and Captured states. Issue reserves it;
the next enabled edge captures; the following edge can transfer. A blocked
captured head prevents another issue. A captured transfer may admit the next
read; a pending return may not. `last_attribute_capture` seals further reads.
Its transfer precedes release; final transfer and ACK may coincide. The caller
proves other consumers finished before supplying that marker.

CE freezes admission, writes, publication, returns, ACKs and cancel drain.
Positive finite wall/source/reservation budgets remain enforced. Invalid context,
generation, row order, premature ACK and illegal transitions fault instead of
recycling storage. Watchdog activity does not count as successful payload progress.

## Cancellation

Cancel discards an unpublished writer and emits SnapshotAborted instead of
successful last-use. A pending read captures before a later discard. A captured
return can discard without a successful transfer. Published slots also require
fault-only abort_ack from cancelled consumers and a drained return before release.
`drained()` covers this controller alone, not source DMA or a render fence.

The source-record link rejects unsupported cancellation and requires external
drain/recreation. It does not turn cancellation into snapshot consumption.

Implementation anchors: [controller](/ip/gpu-v2/src/geometry/record_transport.rs),
[source link](/ip/gpu-v2/src/geometry/source_record_link.rs).
