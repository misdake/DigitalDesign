# GPU command lists

Use `CommandList` to submit several command buffers or prepared slices as one
ordered batch. The Tang Nano 20K command processor runs each segment in order
after a single submission. Use `Gpu::submit` and `Gpu::wait` to submit a single
command buffer directly.

## Submit recorded command buffers

Call `CommandList::new` with the exact number of segments, from 1 to 32. Add
each recorded CB in execution order. `push` seals the CB and takes ownership.

```rust
let mut list = CommandList::new(2);
list.push(first_cb);
list.push(last_cb);
gpu.submit_list(list);
// Independent CPU work can run here.
gpu.wait_list();
```

The list builds a 128-byte-aligned table in SDRAM and references existing
command storage. Each segment spans at most 64 sectors of 128 bytes each.
`wait_list` or a successful `try_wait_list` retires the batch and returns
consumed slots to `Gpu::command_buffer()` for fresh recording. Retire the
current batch before submitting another.

## Replay a prepared slice

Mark each independently selectable draw group and the end of the recording
with `PreparedPart::mark_slice`. A mark returns the current instruction offset
and makes the next nonempty draw emit its World and material setup. Submit a
half-open range `[first, end)` with two marks from the same recording. A mark
counts 32-bit instructions, so the first mark can be inside a sector.

```rust
let mut part = prepared.record();
part.set_state(state.ptr());
let first = part.mark_slice();
part.draw_colored(transform.ptr(), mesh.ptr(), material.ptr());
let end = part.mark_slice();
part.finish();

let mut list = CommandList::new(3);
list.push(setup_cb);
list.push_prepared_range(prepared.ptr(), first, end);
list.push(tail_cb);
gpu.submit_list(list);
gpu.wait_list();
```

In this example `setup_cb` establishes Target and View. Sealing `tail_cb`
ensures the terminal fence and END. The driver checks the marked range and
the installed scene, requires a later segment, and protects the Prepared
registration from changes while the list holds it. Retirement or dropping
an unsubmitted list releases that protection. Keep backing storage and
referenced external inputs valid and unchanged during that interval.

Prepared owners and pools claim their physical SDRAM extents at creation.
Overlapping claims or overlap with a configured frame CB are rejected.
Debug builds validate the selected groups against the installed scene light.

The Box and Rabbit ring demo selects one range in the middle of the stream
and two ranges when the selection crosses the seam. Both ranges go in the
same list. [Ring demo](/systems/cpu-v3-tang-nano-20k/examples/demo_box_rabbit/README.md)

For raw slices, descriptor limits, and hardware behavior, see the
[segmented submission ABI](/ip/gpu-v2/docs/commands.md#segmented-submission).
