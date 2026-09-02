# CPU V3 IP documentation

These documents describe the reusable CPU V3 ISA and processor IP. Board-specific boot, SDRAM,
display, device allocation, and fitted timing belong to the Tang Nano 20K system documentation.

## Documents

- [`isa.md`](isa.md): normative ISA, architectural addressing, fault, and ABI specification for
  revision 0.7. [`isa.html`](isa.html) is the self-contained visual encoding reference. Neither
  document defines Cache or fitted-system policy.
- [`hardware-architecture.md`](hardware-architecture.md): current Stage 12 core, FPU, fetch, and
  cache microarchitecture, including instruction latency and implementation timing context.
- [`cpu_v3_structure.puml`](cpu_v3_structure.puml): maintainable source for the CPU V3 structure
  diagram.

The complete Tang Nano 20K composition is documented in
[`systems/cpu-v3-tang-nano-20k/docs/README.md`](../../../systems/cpu-v3-tang-nano-20k/docs/README.md).

## Version boundary

ISA revision and implementation Stage are independent. Revision 0.7 defines the architectural
instruction and ABI contract. Stage 12 names the current microarchitecture optimization level and
does not change the ISA encoding.
