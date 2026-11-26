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

## Interleave dynamic commands and prepared ranges in one CB

Use `begin_list` on a fresh CB to keep one writable owner for a complete frame.
Record a setup pass, link marked Prepared ranges between closed passes, then
record a tail pass and consume the CB with `finish_list`:

```rust
let mut commands = gpu.command_buffer();
commands.begin_list(3);
// Record and close the setup pass, establishing Target/View and entry state.
commands.link_prepared_range(prepared.ptr(), first, end);
// Record and close the tail pass in the same commands owner.
let list = commands.finish_list();
gpu.submit_list(list);
gpu.wait_list();
```

The exact segment count includes every nonempty dynamic interval, each linked
range, and the final dynamic interval with END. Consecutive links do not insert
empty dynamic intervals. Each interval must fit within 64 sectors, including
its offset inside the first sector; an unaligned interval can therefore hold
fewer than 2,048 instructions. `command_words()` counts only the dynamic CB's
16-bit words, excluding linked commands. No command or push bytes are copied.

Linking applies the same entry checks and immediate Prepared pin as
`push_prepared_range`. The next dynamic draw reloads World. A shared-state range
preserves the checked material/response; an ordinary range invalidates them.
`finish_list` supplies the terminal fence and END even without a tail pass.
Reset or Drop before finishing releases the unsubmitted table and its pins.
An unfinished linked CB cannot be submitted directly or pushed into another list.

The resulting list owns one configured CB slot. With two slots, prepare the
entire next frame while the previous list is in flight, then retire/present
the previous frame before submitting the next one. The application remains
responsible for its surface and external-input lifetimes.

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

## Share one state across independent slices

Call `part.share_state()` before recording any draw to declare a homogeneous
ordinary World stream. Its first draw defines the required native CP material
and lighting response. Later draws may vary transform and uniform tint, but
changing CP-consumed material/response fields or recording a billboard fails.

`mark_slice()` then requires the next draw to upload its World packet while
preserving the shared material. The recorded slices contain no material SET,
full-render fence, or restoration segment. Establish the matching material and
response with a normal draw in the setup CB. Both release and debug builds
check that entry state before a slice is added. An ordinary prepared range
invalidates the list's shared-state proof; a new recorded CB can establish it
again. Part lifetimes and marked endpoints use the same checks as ordinary
prepared slices.

This is useful for a scene organized around one atlas and render state. Native
RGB triangles bypass sampling and may share the installed texture state with
UV triangles. Keep actual atlas-page or opaque/transparent state transitions in
the application's normal command stream.

For raw slices, descriptor limits, and hardware behavior, see the
[segmented submission ABI](/ip/gpu-v2/docs/commands.md#segmented-submission).
