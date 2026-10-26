# GPU profiler follow-up

These are pending tasks, not implemented interfaces or board qualification.
The existing simulation tool is described in [README.md](README.md) and the
[session specification](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md).

## 1. Freeze a pending submission in software

- [ ] Add a driver-owned hook after the preceding GPU submission is fully drained,
  the new command buffer and inputs are sealed, and cache maintenance is complete,
  but before either COUNT or START is sent to Device4. Do not change GPU RTL.
- [ ] Let the second button toggle pause/resume. While paused, the first button
  requests a dump; pausing alone must not start a transfer. Keep Display refreshing
  the last presented framebuffer.
- [ ] Pin the command buffer and its complete input dependency set, including the
  actual UI page, until resume. Do not record, recycle or mutate these resources
  during a dump. Recheck visibility and lifetime before submitting the same buffer.
- [ ] Identify the displayed frame separately from the pending submission. Exclude
  debug pause and transfer time from GPU timing; reset input/time baselines on resume.

Acceptance: repeated paused dumps have identical GPU input bytes and identities;
Display continues, GPU does not render, and resume submits the captured buffer once.
Exercise both input/UI pages and reused command-buffer addresses.

## 2. Export the complete replay input

- [ ] Define a versioned snapshot containing submission base/count, command bytes
  and padding, object addresses/lengths/hashes, and software/RTL/layout/ROM identities.
  Export the referenced mesh, meshlets, textures, atlas, UI and uniform/state inputs.
- [ ] Require self-contained state bindings, or save the driver's retained state
  shadow and an explicit restoration preamble outside the measured submission.
  DRAM inputs alone cannot recover omitted global/material/target/UI register state.
- [ ] Include output preimages for LOAD or incremental rendering. Omit them only
  when the captured stream initializes all target contents that it later reads.
  A complete CLEAR permits omission of the corresponding old plane contents.
- [ ] Use binary chunks with snapshot ID, sequence, length and checksum. The current
  UART path is TX-only; do not assume host requests, ACKs or automatic retransmission.
  A later button press may repeat the frozen snapshot.
- [ ] Permit verified host-side reuse of immutable assets. Unknown or mismatching
  assets must still be transferable; a reference without matching bytes is incomplete.

Acceptance: detect truncation, missing objects, corruption and mismatched assets;
reconstruct exact input bytes without modifying the frozen GPU resources.

## 3. Replay a board snapshot on the production GPU and MC

- [ ] Load a validated snapshot into a bounded cold-start GPU+MC harness, issue the
  captured Device4 submission and attach the existing read-only observer. Check
  output pixels and memory against the captured result when available.
- [ ] Preserve input and DUT/MC hashes before simulation. Use isolated output
  directories; companion files must never overwrite an exported DUT or its inputs.
- [ ] Keep logical frame IDs, submission IDs and command occurrences distinct;
  exercise repeated runs and A/B/A command-buffer address reuse.
- [ ] Label cold-start and warm-state runs separately. A DRAM dump alone does not
  recover arbitrary internal cache, FF or BSRAM state. GPU+MC timings do not include
  production CPU/Display contention; add that workload explicitly when needed.

Acceptance: repeated cold runs conserve tokens and reproduce output memory and
timing. Each simulator has cycle and wall-clock limits. Do not infer board timing
or rendered correctness from observer-only smoke checks.

## 4. Reduce transfer time after correctness

- [ ] Measure CPU memory-read time, serialization time and actual UART throughput
  separately. First reduce the dependency set and reuse verified static assets;
  consider optional lossless block compression with a raw fallback.
- [ ] If rebuilding the bitstream is authorized, test the existing UART at
  1/2/3 Mbps by changing its fixed divider. Validate the USB bridge and sustained
  CPU feeding rate with checksums before selecting a baud rate. No UART DMA is
  required by this design.

Acceptance: report measured end-to-end time and exact restored bytes. The current
115200-baud 8N1 line takes at least about 16.7 seconds for a 192000-byte plane;
proposed faster rates are unverified until tested on the actual link.
