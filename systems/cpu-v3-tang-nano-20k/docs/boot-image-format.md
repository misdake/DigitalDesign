# CPU V3 boot image format version 3

All multibyte integers are little-endian. Flash offsets are byte offsets from
the beginning of the package. SDRAM destinations are physical 16-bit-word
addresses. Section sizes and alignments are bytes.

The format deliberately has two metadata levels. Immutable Stage0 understands
only the fixed 64-byte descriptor. Stage1 understands the manifest and section
records, so later section features do not force a Stage0 replacement.

## Package layout

| Region | Placement |
| --- | --- |
| Boot descriptor | byte `0`, exactly 64 bytes |
| Manifest header | byte `64`, exactly 48 bytes in version 3 |
| Section records | immediately after the manifest header, 32 bytes each |
| Loaded section data | 256-byte-aligned; Stage1 is placed first |
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
| 20 | 4 | Stage1 Flash offset |
| 24 | 4 | Stage1 file size |
| 28 | 4 | Stage1 memory size |
| 32 | 4 | Stage1 physical destination word |
| 36 | 2 | Stage1 `CSEG` |
| 38 | 2 | Stage1 entry offset |
| 40 | 2 | Stage1 `DSEG` |
| 42 | 2 | Stage1 initial stack offset |
| 44 | 4 | manifest Flash offset |
| 48 | 4 | manifest size |
| 52 | 4 | reserved zero (Stage1 CRC-32 before version 3) |
| 56 | 4 | reserved zero (descriptor CRC-32 before version 3) |
| 60 | 4 | physical word destination for the mirrored Stage0-to-Stage1 descriptor |

Real hardware has no direct Flash read path, so Stage0 DMAs the 64 descriptor
bytes from Flash offset `0` into the reserved physical scratch range at word
`0x40` and validates the SDRAM copy: magic, version, target, every Flash and
SDRAM extent, and the Stage1 entry. It then DMAs Stage1 to its destination,
mirrors the 64 descriptor bytes from the scratch range into offset `0x0100`
of the Stage1 data segment, performs complete D-cache invalidation through the
semantic compiler barrier, sets `DSEG` and the stack pointer, then executes the
adjacent `ICACHE_INVALIDATE_ALL_DELAYED; JSEG` terminal handoff. The packer
reserves both the scratch range and that handoff range against every
loadable section.

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

## Host input manifest

`cpu-v3-pack` deliberately uses a small dependency-free line format so packaging
continues to work offline. `#` starts a comment. Numbers are decimal or
`0x`-prefixed hexadecimal. Paths are relative to the manifest.

```text
format 1
target tang-nano-20k
stage1-section stage1
stage1-entry 0x0001 0x0100 0x0002 0xf000
application-entry 0x0003 0x0200 0x0004 0xf000

load stage1 0x00010100 rx 32 4096 stage1.bin
load code   0x00030200 rx 32 32768 game-code.bin
load data   0x00044000 rw 32 16384 game-data.bin
zero bss    0x00048000 rw 32 8192
```

The columns after a `load` name are physical destination word, flags,
destination alignment, occupied memory bytes, and source file. A `zero` line
omits the source file. Section names and file paths may not contain whitespace
in host-manifest format 1. This text format has its own version independent of
the binary boot-image version.
