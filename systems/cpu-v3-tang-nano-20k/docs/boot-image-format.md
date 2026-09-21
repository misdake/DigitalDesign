# CPU V3 boot image format version 4

All multibyte integers are little-endian. Flash offsets are byte offsets from
the beginning of the package. SDRAM destinations are physical 16-bit-word
addresses. Section sizes and alignments are bytes.

A single first stage runs the whole boot. It understands both the fixed
64-byte descriptor and the extensible section manifest, so the board no longer
carries a separate Stage1 image.

## Package layout

| Region | Placement |
| --- | --- |
| Boot descriptor | byte `0`, exactly 64 bytes |
| Manifest header | byte `64`, exactly 48 bytes in version 4 |
| Section records | immediately after the manifest header, 32 bytes each |
| Loaded section data | 256-byte-aligned, in manifest order |
| Unused padding | `0xff` |

The package is relocatable in external Flash. Every stored offset is relative
to its package base. Board programming metadata chooses that base only after
the FPGA configuration region has been characterized.

## Boot descriptor

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | ASCII `CPU3BOOT` |
| 8 | 2 | format version |
| 10 | 2 | descriptor size |
| 12 | 4 | target identifier |
| 16 | 4 | complete package size |
| 20 | 24 | reserved zero (Stage1 load/entry fields before version 4) |
| 44 | 4 | manifest Flash offset |
| 48 | 4 | manifest size |
| 52 | 12 | reserved zero (CRC-32 and Stage1-handoff fields before version 4) |

Real hardware has no direct Flash read path, so the first stage DMAs the 64
descriptor bytes from Flash offset `0` into the reserved physical scratch range
at word `0x40` and validates the SDRAM copy: magic, version, target, and every
Flash and SDRAM extent. It then DMAs the manifest into its own data-segment
buffer, validates it, DMAs every selected application section to its
destination, performs complete D-cache invalidation through the semantic
compiler barrier, sets `DSEG` and the stack pointer, then executes the adjacent
`ICACHE_INVALIDATE_ALL_DELAYED; JSEG` terminal handoff. The packer reserves the
descriptor scratch range against every loadable section.

## Manifest header

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | ASCII `CPU3SECT` |
| 8 | 2 | format version |
| 10 | 2 | manifest header size |
| 12 | 2 | section record size |
| 14 | 2 | section count |
| 16 | 4 | complete package size |
| 20 | 2 | application `CSEG` |
| 22 | 2 | application entry offset |
| 24 | 2 | application `DSEG` |
| 26 | 2 | application initial stack offset |
| 28 | 4 | section table offset from manifest start |
| 32 | 4 | section table size |
| 36 | 4 | reserved zero (section-table CRC-32 before version 3) |
| 40 | 4 | reserved zero (manifest CRC-32 before version 3) |
| 44 | 4 | reserved zero |

## Section record

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 2 | kind: `1=Load`, `2=Zero` |
| 2 | 2 | flags: bit 0 read, bit 1 write, bit 2 execute |
| 4 | 4 | source Flash offset; zero for `Zero` |
| 8 | 4 | destination physical word address |
| 12 | 4 | file byte size; zero for `Zero` |
| 16 | 4 | occupied memory byte size |
| 20 | 4 | required destination alignment in bytes |
| 24 | 4 | reserved zero (file CRC-32 before version 3) |
| 28 | 4 | reserved zero |

`Load` copies its file bytes and zero-fills `memory_size - file_size`. If the
file size is odd, the high byte of the final SDRAM word is zero. `Zero` writes
zero over the complete memory extent. Physical memory extents may not overlap.
An entry point must lie in the file-backed portion of an executable `Load`
section, not merely in its zero-filled tail.

The first stage holds the manifest in a 192-word static buffer at data-segment
word 0 while it parses the manifest and loads sections (the 64-byte descriptor
scratch at word `0x40` lives inside that range). The packer therefore reserves
physical words `0..=191` against every section, so no loadable section may be
placed there.

The generated `boot_selection` module supplies the S1 and S2 code/data
segments and entry offsets. The first stage loads only the section whose
executable destination matches the reset-selected application; every other
section is skipped. Each application is currently emitted as a single
executable `Load` section, so this per-entry selector identifies it completely.

## Host input manifest

`cpu-v3-pack` deliberately uses a small dependency-free line format so packaging
continues to work offline. `#` starts a comment. Numbers are decimal or
`0x`-prefixed hexadecimal. Paths are relative to the manifest.

```text
format 1
target tang-nano-20k
application-entry 0x0003 0x0200 0x0004 0xf000

load code   0x00030200 rx 32 32768 game-code.bin
load data   0x00044000 rw 32 16384 game-data.bin
zero bss    0x00048000 rw 32 8192
```

The columns after a `load` name are physical destination word, flags,
destination alignment, occupied memory bytes, and source file. A `zero` line
omits the source file. Section names and file paths may not contain whitespace
in host-manifest format 1. This text format has its own version independent of
the binary boot-image version.

The fitted system does not maintain this detailed manifest by hand. Its
`boot-applications.conf` contains only:

```text
format 1
s1 rcc/boot-demo.rs
s2 rcc/display-demo.rs
```

C3 restores the migrated Q16.16 FPU display demo to S2. It uses the compiler's
scalar, vector, and special-function lowering directly and contains no software
trigonometric table.

The build compiles those sources into derived S1/default and S2 slots and emits
`boot.cpu-v3-manifest` beside the section binaries. That generated manifest is
the independently repackable input to `cpu-v3-pack`; `boot-project.map` records
the source, entry, destination, size, and fingerprint for each slot, while
`boot-selection.generated.rs` records the exact constants compiled into the
first stage. Replacing either application requires changing only the
corresponding line in `boot-applications.conf`.
