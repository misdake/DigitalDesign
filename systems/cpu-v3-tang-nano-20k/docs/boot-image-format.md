# CB06 cold-boot image

The system accepts one sequential image at absolute Flash byte 0x100000. It
copies code and resources into final DRAM addresses and materializes zero
regions before enabling Display and releasing the CPU. All integers are little
endian; persisted addresses and lengths are **bytes**.

## Stream encoding

| Bytes | Encoding |
| --- | --- |
| First 4 | ASCII CB06 |
| Each command, bytes 0..3 | u32 physical DRAM byte address |
| Each command, bytes 4..7 | bit31=ZERO, bits30..0=byte length |
| Immediately after COPY | Exactly length bytes of payload |
| END | An all-zero 8-byte command, with no payload |

COPY has bit31=0 and a nonzero even length. ZERO has bit31=1 and a nonzero
even length, without payload bytes. Both require an even destination. There is
no source offset, section table, section count, image-length field, checksum,
compression, relocation, dynamic entry or runtime loader command.

The hardware checks the signature, command shape, even alignment, DRAM capacity
and exclusion of Depth byte [0x460000,0x490000). High bits are checked before
narrowing to the 22-bit word address, and the end calculation retains its carry.
The existing SPI reader rejects commands exceeding the 8-MiB Flash capacity.
A malformed stream can fail after partial writes; there is no rollback.

The host packer checks full layout ownership, nonoverlap, required code/static
initialization and the complete Flash image size. Hardware does
not duplicate these checks. A corrupted payload can execute incorrectly, and a
hand-made stream missing required code can reach END: trusted host-generated
input and programmer Verify are the intended workflow.

## Host layout and resources

| DRAM byte range | Ownership |
| --- | --- |
| [0,0x4000) | Protected static arena; ZERO only the compiler-allocated prefix |
| [0x4000,0x20000) | Protected low heap/stack window |
| [0x20000,0xe0000) | Resource allocations |
| [0xe0000,0x100000) | One code COPY at 0xe0000; unused tail remains reserved |
| [0x100000,0x400000) | Resource allocations |
| [0x400000,0x460000) | Reserved framebuffer slots; boot leaves both untouched |
| [0x460000,0x490000) | Reserved Depth; never initialized by boot |
| [0x490000,0x4b0000) | Driver-owned command/object arena; optional COPY/ZERO resources rejected |
| [0x4b0000,0x7df800) | Fixed GPU asset allocations |
| [0x7df800,0x7e0000) | Reserved UI pages; optional resources rejected |
| [0x7e0000,0x800000) | Second driver command/object arena; optional resources rejected |

Code is nonempty, even, and at most 128 KiB. A=0x400000 and B=0x430000, with
0x30000 bytes per slot. Depth is cleared by each GPU Pass. Optional resource
COPY/ZERO commands must fit one resource interval and cannot overlap.
Odd-byte files acquire one zero pad byte as part of their allocation.

The project file retains format 2 and one application, with optional entries:

~~~text
format 2
application rcc/system-display-test.rs
copy MESH 0x100000 assets/mesh.bin
zero 0x4b0000 4096
~~~

Paths are relative to the project directory. COPY symbols are unique upper
snake case. The build generates boot_resources.rs before compilation; an RCC
application can import mod boot_resources and use NAME_BYTE_ADDRESS_LOW/HIGH
and NAME_WORD_ADDRESS_LOW/HIGH u16 constants. Full addresses are split explicitly
because this RCC frontend has no u32 type; no compiler or far-pointer ISA is added.
RCC still initializes nonzero statics and sets SP=0xc000.

Device2 is unassigned. The cold loader has no software-programmable DMA register
bank; it starts after PLL/SDRAM readiness and is controlled by the CB06 stream.

The compiler publishes the exclusive end of its static allocation in words.
With the system's fixed data base of zero, the packer emits one ZERO of twice
that word count; a zero count omits the command. The count includes module and
standard-library statics and aggregate padding, including statics whose functions
are later discarded by linking. The unused arena tail is left untouched and
remains protected from resource allocations. The 8192-word capacity is unchanged.

The system module resolver checks `<application-directory>/<name>.rs` first,
then `<system>/rcc/<name>.rs` only when the local read reports NotFound. All
other I/O/UTF-8 errors and all selected-source compile errors remain errors;
there is no fallback after a bad local source. The generated `boot_resources`
module takes precedence over both directories. Transitive modules use the same
lookup policy, allowing independent examples to share `gpu`, `time` and their
dependencies without source copies or a compiler-specific example search path.

Cargo tracks the application/config/resource inputs and each actual module.
When a shared module is selected, it also tracks the existing application
directory so creating a local override invalidates the build. It does not watch
a nonexistent local file on every successful build. The default project and
application selection are unchanged; examples select their own project explicitly.

The build emits application.v3bin, application.static-words, cpu-v3-boot.bin, cpu-v3-boot.map and
boot_resources.rs only in Cargo OUT_DIR. cpu-v3-boot-assets materializes these
exact artifacts. cpu-v3-pack application.v3bin --project boot-applications.conf
independently reproduces the package, using the same resource files and the
adjacent application.static-words file (a decimal word count followed by a newline).
For externally compiled binaries, --static-words N supplies that count explicitly.
Missing or malformed metadata is an error; raw instructions do not encode the
static allocation extent. Keep each binary and its generated length file together.
The CPU instruction bytes are unchanged by this packaging metadata. Omitting
--project packs code plus mandatory initialization only. Asset SHA-256
identities can be recorded with the offline image bundle.

The explicit `CPU_V3_BOOT_EXAMPLE=gpu_demo` build selects the independent demo
without editing the default project. Its single producer generates immutable
room/texture/atlas assets in OUT_DIR; generated roots extend boot_resources.
The materializer also exports those resources, their identity manifest and a
resource-only `demo-repack.conf` for independently packing the compiled code.
The [demo build entry](/systems/cpu-v3-tang-nano-20k/examples/gpu_demo/README.md#build-and-flash-handoff)
restores the caller's environment and requires matching generated/repacked CB06
SHA-256. Generated resources are tracked by producer inputs rather than treated
as new Cargo input files. The ordinary application selection is unchanged.

## Transfers and completion

The loader reads the signature and one 8-byte command at a time, waiting for
each SPI transaction's done before interpretation. COPY performs one continuous
SPI read of its payload. A byte is consumed exactly once on valid/ready.
ZERO bypasses Flash and selects a constant zero word.

Both operations use the existing arbiter DMA narrow write client. The loader
holds one 16-bit word, never issues a successor before the current response,
and advances its target and remaining count only on a successful response.
Counter contents after entering FAILED are unspecified; an error response never
authorizes a subsequent write, command or completion publication.
No additional arbiter owner, phase mux, FIFO, 32-byte buffer or burst writer
is added. SPI done and the final write response are independent; both must
complete COPY. Per-transaction SPI done state is cleared at each new start.

~~~mermaid
flowchart LR
  F[Flash: command and inline data] --> R[SPI byte reader]
  R --> W[One-word COPY/ZERO writer]
  W --> A[Existing DMA arbiter client]
  A --> M[SharedSdramPort / gear / MC]
  M -->|write response| W
  W --> E[END read completed / load_done]
  E --> D[Display prime / frame-start acknowledgement]
  D --> C[boot_done / CPU release]
~~~

load_done means END was read successfully, all previous writes completed, and
no boot transaction remains. It is not an integrity result. The loader then
permanently stops issuing memory requests and enables Display prefetch.
The normal arbiter remains connected throughout.

Display starts scanout from A at a full frame boundary after both complete
initial black row-group markers have crossed into the pixel domain. Those
markers use the normal publication/release handshake without DRAM reads or
line-buffer data writes. Its sticky started_ack
returns through two logic-domain synchronizer stages. CPU release additionally
requires cache metadata cold-ready and no startup error. The fixed entry is
CSEG7:PC0, DSEG0/1/2/3. Generic CPU IP still starts in CSEG0.

The acknowledged initial ownership is A=Front (virtual black), B=Free and no
Pending. Neither framebuffer nor Depth is initialized by boot. The first
NEXT_SWAP consumed at a normal frame boundary activates real DRAM fetches,
including a same-address present of A. Real content remains black until two
complete real groups are available at a frame origin; incomplete groups are
retained rather than consumed as black. Software must fully initialize the
first target and wait for completed GPU writes before presenting it. GPU DRAM masters wait for boot_done;
GPU table/binding readiness independently gates GPU submission. Boot does not
initialize GPU internal memory or wait for gpu_ready.

## Failure and clock settings

Errors are sticky and stop new requests without releasing the CPU. An already
accepted memory request retains response ownership; a missing terminal cannot
be reported as drained. There is no automatic retry, software reset, error
UART packet or runtime boot restart. Later Display errors use its normal health
path rather than rolling back boot.

LEDs show 000001 while booting and 11xxxx on failure: 1=signature/command,
2=address/length, 3=Flash, 4=memory/Display, 5=no-progress timeout. Coincident
Flash error precedes memory error, then command/range, Display and timeout.
One 24-bit progress watchdog is shared across active phases, including Display
startup; at 54 MHz this is about 311 ms without progress. Initial ready waiting
has no timer, and there is no separate total-boot deadline.

The default fitted SPI divider remains 2: nominal 13.5-MHz active SCK from the
54-MHz logic clock. CPU_V3_SPI_HALF_PERIOD=1 selects the optional nominal
27-MHz single-line 03h candidate at build time; only 1 and 2 are accepted.
SCK and byte transfer pause under backpressure. This does not imply a factor
of two improvement in whole-boot latency.

The SPI half-period counter is sized from its specialization rather than
reserving 32 bits: the default divider 2 requires one bit. Its terminal
comparison, SCK edges, backpressure and final-byte completion are unchanged.

Protocol simulation uses a 6.5-ns Flash output-delay reference without board
or FPGA I/O delay. It is not physical timing qualification for an unidentified
part. The reader cannot exceed nominal 27-MHz SCK using this 54-MHz single-edge
structure. Quad/QE, a faster clock domain and new buffering are absent.
