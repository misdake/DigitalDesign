# Tang Nano 20K external Flash layout

The current board's runtime SFDP probe reports an 8-MiB device. Its JEDEC ID is
`EF 40 17`; this is a Winbond-family 64-Mbit part even though some board
material lists a different vendor. Software therefore binds this concrete
Tang Nano 20K variant to 8 MiB rather than inferring capacity from a generic
board name.

The first 1 MiB is reserved for FPGA configuration. The G16 package begins at
byte `0x100000` and may occupy at most 7 MiB:

```text
0x000000 .. 0x0fffff  FPGA configuration and erased padding
0x100000 .. 0x7fffff  relocatable G16 boot package
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

`g16-pack --configuration-bin design.bin --flash-image complete.bin` rejects a
configuration larger than the reserve, fills the unused gap with `0xff`, and
places the validated package at `0x100000`. Package-internal Flash offsets stay
relative to that base so Stage0 needs only one fixed package-base constant.

For development, the FPGA may still be programmed only to volatile SRAM while
the package is written separately at `0x100000`. Programmer operation 32
(`exFlash C Bin Erase,Program,Verify`) accepts a raw binary through the
misleadingly named `--mcuFile` option plus `--spiaddr`;
bulk erase is never part of the normal workflow. Writing is enabled only after
the generated file, target ID, start address, capacity, and sector extent have
all passed host-side checks.
