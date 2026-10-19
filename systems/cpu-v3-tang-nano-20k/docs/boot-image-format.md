# Single-application Flash format v5

The FPGA hardware loader accepts one fixed-destination application. The build compiles the sole `application` in `boot-applications.conf` from RCC and produces `application.v3bin`, `cpu-v3-boot.bin` and `cpu-v3-boot.map` only in Cargo `OUT_DIR`. `cpu-v3-boot-assets` materializes those exact bytes. `cpu-v3-pack application.v3bin -o cpu-v3-boot.bin` independently recreates the container; it accepts no section table or dynamic entry.

All header fields are little endian. The package starts at absolute Flash byte `0x100000`, after the 1 MiB configuration reserve. The image is independent of FPGA application size and can be updated without rebuilding gateware.

| Package byte offset | Size | Value |
| --- | ---: | --- |
| 0 | 8 | ASCII `CPU3BOOT` |
| 8 | 2 | Format version 5 |
| 10 | 2 | Header size 32 |
| 12 | 4 | Code byte length |
| 16 | 16 | Reserved, all zero |
| 32 | 96 | Alignment padding, emitted as erased bytes |
| 128 | code length | One contiguous executable payload |

The code length must be nonzero, even and at most 131072 bytes. Header validation also proves the payload is within the 8 MiB Flash device. There is no arbitrary destination, alternate application, section loop, tail-zero section, executable relocation, or reset-time button selection. Older package versions are rejected before any SDRAM write.

The fixed board entry is CSEG=7, PC=0, data pages 0/1/2/3, with application stack initialization SP=0xc000. Code is copied to physical 16-bit word `0x70000`. Exactly 8192 words at physical zero are cleared before copying code. This is the 16 KiB compiler static arena; heap, stack and framebuffer initialization retain their existing owners. RCC still initializes nonzero static elements; zero elements now have an explicit Boot owner even with nonzero initial SDRAM contents.

The fixed physical-memory ownership uses 16-bit word addresses:

| Word range | Owner / initialization |
| --- | --- |
| `0x000000..0x001fff` | 16-KiB static arena; loader clears every word |
| `0x002000..0x00ffff` | Remaining low data window; application storage/heap/stack initialization |
| `0x070000..0x07ffff` | One 128-KiB code segment; loader writes only the declared payload |
| `0x200000..0x217fff` | Framebuffer A slot, including padding |
| `0x218000..0x22ffff` | Framebuffer B slot, including padding |

SP=0xc000 is a logical word pointer in the fixed data-page mapping. Boot does
not clear heap, stack or framebuffers. Their existing application owners remain.
Other SDRAM ranges remain general memory; this table does not reserve them.

The CPU is held before it can issue an instruction, data request, or device write. The loader validates the 32-byte header, clears the static arena and copies code through the arbiter's existing narrow write client. Every word advances only after its successful memory response. The last code response and Flash completion must both occur before CPU release. The I/D-cache metadata cold state is invalid, queue/BTC valid state is empty, and BTC pair ownership has an initialized bijection. No CPU dirty cache data exists to flush at this handoff.

Malformed headers, invalid lengths, Flash error, memory error (including the last response), or progress timeout enter a permanent fail-stop state. LEDs show `11xxxx`, where `xxxx` is 1=header, 2=length, 3=Flash, 4=memory, 5=timeout. The CPU remains held. This hardware status replaces the old software loader's UART failure frame. An erased/truncated stream must fail or time out; it cannot release partial code.

The loader holds a write request's address and data until acceptance, then waits
for its response before advancing. Flash completion may precede or follow the
last code-write response; both are required for success. A Flash error takes
priority over a coincident memory error. Success or failure remains latched for
the cold-start interval, and toggling the startup-ready gate cannot restart it.

The independent host installer is a functional model, not a cycle or pin model. RTL tests separately cover real SPI shifting, nonzero SDRAM, static initialization, final-write ownership, first fetch and fixed entry. Resource and startup performance require source-matched complete-system measurement; no board result is implied by these contracts.
