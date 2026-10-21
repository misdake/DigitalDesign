# Standalone memory interface

## Masters and address units

The descriptor-based GPU exposes three main-system masters:

| Master | Traffic | Direction |
| --- | --- | --- |
| RO | Geometry/config loader and texture refill | Read-only, transaction-level shared owner |
| Framebuffer read | Color/depth tile refill | Read-only |
| Framebuffer write | Dirty color/depth sectors | Write-only |

Addresses are 22-bit halfword units, covering 8 MiB. Data beats are 64 bits.
Count codes 0/1/3 select 32/64/128 naturally aligned bytes; code 2 is invalid.
A 128-byte sector carries 16 beats. The loader selects sizes from remaining
length and natural boundaries; a tail shorter than 32 bytes still reads 32 bytes
and copies only its valid qwords into local storage.

Texture and framebuffer internally use 32-bit byte sector addresses. Adapters
check alignment/range, then divide by 2 for the main-system address. Framebuffer
writes stream from the cache without a second whole-burst payload FIFO.

## Request, data and terminal ownership

A presented request holds valid, address and count until request-ready accepts
it. This transfer reserves the master and return destination. Responses have
no ready input: the destination must already have room for the whole admitted
read. Each accepted response belongs to the locked owner through actual
last/error, regardless of compute CE.

At the actual main-system pins, distinguish:

| Event | Pin condition |
| --- | --- |
| Read data beat | `ro_response_valid` or `fb_r_response_valid`, with no response error |
| Successful read terminal | The same valid beat also asserts `*_response_last`; its index is the expected final beat |
| Write data beat | Accepted `fb_w_data_ready` while the cache offers its held data |
| Successful write terminal | `fb_w_response_valid && fb_w_response_last && !fb_w_response_error`, after all 16 write beats |
| Error terminal | A response-valid edge with response-error; it can terminate early |

Read last and read success terminal share an edge here. Write last data and write
terminal response are different events; the response may coincide with the last
accepted data beat. The router enters ACTIVE after request acceptance, so these
physical pins must not return a response on that acceptance edge. Separate
byte-facade test interfaces may have other explicitly documented edge rules.

Framebuffer write data holds through data-ready stalls. A write source retains
all remaining beats and must sustain the accepted burst's data contract; the
physical controller cannot accept arbitrary mid-burst valid holes. The final
write data beat does not itself acknowledge the transaction. Terminal response
ends the owner. Earlier writes survive an error.

RO arbitration is round-robin per transaction between geometry and texture.
An optional third control client supports command fetch, staging DMA and
directory reads in the same router; it adds no physical memory master.
It locks that choice until actual last/error; it never interleaves owners at
beat granularity. Framebuffer read/write have their own master ownership. The
vendor arbiter controls contention between all system clients.

## Pause and clocks

Compute CE controls scheduled arithmetic and its owned synchronous returns.
It does not gate accepted memory requests, response handling, loader DMA writes,
completion/page publication or framebuffer maintenance. An offered request also
cannot disappear merely because compute pauses.

A composition advances the shared memory service exactly once per wall edge.
Several clients may submit/poll that service, but they do not each tick the
physical controller. Host service-queue acceptance and physical master acceptance
are distinct events; [reference memory adapters](/ip/gpu-v2/docs/references/memory.md) define
their own interface.

## Errors and cancellation

Abort stops new computation and future loader work. A request already presented
remains stable until acceptance, then drains to terminal. Router faults cancel
future loader work; only the locked owner receives the current response.
Malformed responses fault instead of silently changing ownership.

No transactional rollback is provided for earlier local DMA or framebuffer
writes. Reset must be coordinated across the GPU, arbiter, bridge and physical
controller, after external ownership is safely handled. A local reset alone
cannot free an externally accepted transaction.

## Shared-system boundary

The Gowin vendor crate owns arbiter, adapter, bridge, controller and pin model.
The CPU board configuration enables one reserved successor request and
other-bank preparation; it does not make the framebuffer's four sectors an
indivisible group. CPU/GPU/Display contention must use that actual configuration.
A standalone transfer or leaf timing result does not establish board integration.

Implementation anchors: [router and adapters](/ip/gpu-v2/src/system/vliw_memory.v),
[three-master wrapper](/ip/gpu-v2/src/system/vliw_sdram.v),
[vendor interface](/hardware/vendor/gowin/doc/README.md).
