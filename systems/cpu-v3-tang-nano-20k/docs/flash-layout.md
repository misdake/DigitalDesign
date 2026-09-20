# Tang Nano 20K external Flash layout

## Board boot progress

The complete boot system passively exposes the current boot phase on the six
logical LEDs. Phase changes are shown immediately without display delays:

| Logical LEDs | Phase |
| --- | --- |
| `000001` | reset held |
| `000010` | waiting for SDRAM initialization |
| `000100` | boot stage executing |
| `001000` | boot DMA active |
| `100000` | application segment entered before its first LED write |
| `100001` | sticky DMA or CPU fault |

The target wrapper performs any physical active-low conversion. The first
software LED write immediately and permanently takes ownership until reset, so
firmware can report either success or a detailed error code. These patterns are
progress evidence only; only the application's UART frame and system-level
checks establish a successful boot.

The board-level selection latch powers up at `10` (S2), so the configured S2
application boots by default; holding the S1 button (`01`) during reset selects
the slider diagnostic, and `11` is ignored. The current configuration uses the
DDHT slider diagnostic for S1. The CPU/FPU sine, cosine, and circle demo is
parked in `rcc/display-demo.rs` while FPU lowering is unavailable at C0, so the
S2 slot boots the non-FPU `rcc/boot-alt.rs` placeholder and reports the boot
DDHT `0x07` frame; the display demo (test ID `0x0b`) returns to S2 after C3. The S2
slot is loaded at `0007:0200`. The display program renders through cached CPU
stores and cleans D-cache before each vblank framebuffer publication when it is
configured.

The current board's runtime SFDP probe reports an 8-MiB device. Its JEDEC ID is
`EF 40 17`; this is a Winbond-family 64-Mbit part even though some board
material lists a different vendor. Software therefore binds this concrete
Tang Nano 20K variant to 8 MiB rather than inferring capacity from a generic
board name.

The first 1 MiB is reserved for FPGA configuration. The CPU V3 package begins at
byte `0x100000` and may occupy at most 7 MiB:

```text
0x000000 .. 0x0fffff  FPGA configuration and erased padding
0x100000 .. 0x7fffff  relocatable CPU V3 boot package
```

The 1-MiB boundary is conservative and sector-aligned. Gowin UG290 gives 886
KiB as the uncompressed GW2AR-18 configuration size including initialized EBR.
Gowin SUG502 defines an external-Flash programming start-address granularity of
one 4-KiB sector. A locally generated initialized design converted by
`programmer_cli --filestransform 1` was 577,178 bytes.

References:

- <https://www.gowinsemi.com/upload/database_doc/41/document/5b96299fe6aa7.pdf>
  (UG290, configuration file sizes)
- <https://www.gowinsemi.com/upload/database_doc/370/document/5f8e61665e9e2.pdf>
  (SUG502, external Flash start address)

`cpu-v3-pack --configuration-bin design.bin --flash-image complete.bin` rejects a
configuration larger than the reserve, fills the unused gap with `0xff`, and
places the validated package at `0x100000`. Package-internal Flash offsets stay
relative to that base so Stage0 needs only one fixed package-base constant.

For development, the FPGA may still be programmed only to volatile SRAM while
the package is written separately at `0x100000`. This GW2AR target uses
Programmer operation 39 (`exFlash C Bin Erase,Program,Verify thru GAO-Bridge`),
as recommended for Arora software binaries. It accepts a raw binary through the
misleadingly named `--mcuFile` option plus `--spiaddr`;
bulk erase is never part of the normal workflow. Writing is enabled only after
the generated file, target ID, start address, capacity, and sector extent have
all passed host-side checks. The Gowin project CLI exposes this as
`--program-flash 0x100000 FILE`, which validates the explicit byte offset and
range against the target's fitted flash capacity. The programmer sniffs the
file format by extension, so the binary must be named `*.bin`.

For development, one command audits the existing FPGA artifact, materializes
and independently repacks the configured applications, compares their package
hashes, writes the package at `0x100000`, restores the audited SRAM bitstream,
and performs the default application's UART check:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-system -Mode Full -Port COM8 -WriteBootFlash
```

For a stand-alone cold boot, write one validated image containing both the
FPGA configuration and the boot package, then load the audited SRAM image for
immediate validation:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-system -Mode Full -Port COM8 -WriteCompleteFlash
```

`cpu-v3-boot-assets` does not compile a parallel copy of the firmware. It exports
the single boot stage, the S1/S2 application binaries, the package, layout map,
generated selection source, and generated offline pack manifest produced in
Cargo `OUT_DIR`, together with their sizes and fingerprints. The repository
`quick` validation independently repacks those exported section files through
`cpu-v3-pack` and requires byte-for-byte equality with the generated package.

To replace an application, edit only `boot-applications.conf` and change the
`s1` or `s2` source path. Layout, selection, asset names, package metadata, and
the programming inputs are regenerated automatically. `boot-project.map` is
the human-readable audit of the resulting source-to-slot mapping.
