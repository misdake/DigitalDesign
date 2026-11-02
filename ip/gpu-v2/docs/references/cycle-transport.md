# Production command-profile transport cycle model

`system::vliw_cycle` models `gpu_command_dma` and the CONTROL=0
`gpu_vliw_memory_router` selected by `GPU_V2_COMMAND_TRANSPORT`. It does not
execute Device4 commands, geometry, pixel arithmetic or Display. The selected
Verilog remains unchanged; this is an independent register-state implementation.
The older `frontend::vliw_dma::Loader` and `vliw_memory::Boundary` retain their
separate checked/single-owner APIs.

## Calling and ownership

`Transport::outputs(Input)` is a pure observation of old state.
`Transport::clock(Input)` checks the selected production simulation contracts
and commits one wall-clock edge. A composition samples every participant's old
outputs before committing any participant. Compute `ce` qualifies work admission;
it does not stop offers, accepted responses, RAM writes or completion pulses.
`stop` suppresses new work and RAM writes while presented/accepted traffic drains.
Reset is a coordinated external transport reset, not local cancellation.

The caller owns input/command RAM, physical memory, the clock and MC. Apply
`ram_write/address/data` at the same sampled edge, with the owning RAM's reset
and port rules. The transport contains only descriptors, tags and cursors; no
burst payload or framebuffer copy is allocated. `Dma` and `Router` also expose
their own pure-output/clock APIs for explicit composition.

`ContractError` terminates an invalid simulation run. These checks correspond
to the command profile's passive RTL monitors. They do not provide the generic
checked profile's error terminals, recovery or rollback. All preceding legal
edges remain committed; no invalid-edge recovery behavior is specified. The
command-profile fault outputs remain zero, as in the selected RTL; outer Device4
sticky transport status is outside this model.

## Ports and storage lifetimes

| Interface | Units and transfer | Owner release |
| --- | --- | --- |
| Work | CE-qualified valid/ready, 22-bit halfword source, 7-bit destination, size 0/1/2 = 32/64/128 bytes | Last associated response, not last request issue |
| Mesh work | 32-byte header followed by 1..8 128-byte payload blocks; explicit page/slot | Actual mesh return completion publishes its slot |
| Command-cache fetch | 128-byte-aligned source, selected cache bank | Final received word publishes cache_done/bank |
| DMA RAM | 9-bit index of a 64-bit destination word | Each accepted read word writes directly; publication follows data |
| RO | DMA/texture transaction arbitration, held offer and two ordered accepted return tags | Correct LAST; returned owners never interleave beats |
| Framebuffer | Byte-sector address internally, two same-direction accepted 128-byte descriptors | Separate terminal cursor; later write data may precede an older ACK |
| Physical masters | RO, FB read, FB write; 22-bit halfword addresses, 64-bit payload; count 0/1/3 = 32/64/128 bytes | Each actual terminal, independently of compute CE |

Presented RO requests hold owner/address/count until request-ready. A new
accepted descriptor cannot own a return on that same edge. Read sinks must
reserve the entire admitted payload because physical read returns have no
ready input. Framebuffer write data must be valid when consumed and stable
while blocked; last write data is distinct from the terminal response.
Simultaneous request acceptance, old-owner completion and last-data/ACK use
the same old state, including RAM destination and return-tag selection.

The byte-to-halfword conversion is applied once. Full-byte sectors must be
128-byte aligned and below 8 MiB; supported halfword bursts have natural
alignment. The model retains the selected register widths and wrap behavior.
Producer resource/extent validation remains outside this transport.

Physical `Response` keeps valid/LAST/error/data explicit even on invalid
cycles. The transport is a two-state control model; backing validity and
native pad knownness remain the physical service's responsibility. Unowned
descriptor metadata is not a valid payload and must not be consumed. Reset
does not imply that GPU RAM or framebuffer contents were initialized.

## Bounded verification entry

From the repository root, with an isolated `CARGO_TARGET_DIR`:

```powershell
cargo test --release -p gpu-v2 --test vliw_cycle_transport -- --include-ignored --nocapture --test-threads=1
```

The explicit ignored checks require `IVERILOG_EXE` and `VVP_EXE`. They compile
the original production command DMA/router with the command macro and CONTROL=0.
The test wrapper only connects ports. No alternate functional GPU or fixed
compute delay is used. Public outputs are compared at every sampled edge;
initial unreset outputs and invalid DMA offer address/count are not asserted
as defined data. Other direct payload buses are compared even when invalid.

The fixtures cover two logical jobs/return owners, both mesh pages/cache banks,
nonzero uniform-bank publication, request stalls, CE0 returns, active global
reset, stop/drain, early data with late write ACK and same-edge accept/terminal.
Independent destination and raw-memory checks verify every completed payload.
Actual compiled RTL mutations change CE cursor progress, return owner,
request-ready admission and write terminal timing; a compile failure is not
counted as a detected behavioral mutation. Invalid peer framing/error/holes
must fail the production monitors rather than succeed as a repaired transfer.

The native fixture connects the GPU model live to the existing production
arbiter and one native endpoint/gear/controller/pin model with caller-owned
backing. Accepted-client chain classes follow the board binding. Physical
writes are observed after native edges, not copied at request acceptance.
`backing_write_logic_edges` counts logic ticks with any changed framebuffer
backing word; it is not a count of individual native pin edges.
RTL then replays the recorded native feedback and must reproduce every
request/data/output. This reuses the independently qualified native service;
it is not a new live RTL simulation of the entire SDRAM/board. CPU and Display
requesters are idle, and board GPU request-slot stages are outside this fixture.

Every fixture has a finite edge bound and every tool process has a 30-second
timeout. Raw source, vectors, wrappers, run/compile output and verdicts are
saved in unique directories under the current tree's `target/gpu-cycle-transport`.
Host-throughput samples include native initialization, arbiter settling,
assertions and in-memory trace allocation, while excluding build, RTL tools
and export. They describe this bounded fixture, not a complete GPU/frame rate
or a speedup over another backend; host contention is uncontrolled and RSS
is explicitly unmeasured.
