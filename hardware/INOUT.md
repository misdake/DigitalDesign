# Bidirectional IO roadmap

`InOutSignals` establishes the internal contract needed by a target-specific
bidirectional IO leaf: resolved input, output data, and per-bit output enable.
It does not yet make `inout` a `ModuleIo` direction and cannot be bound to a
board pin.

The next implementation should keep bidirectional behavior at target leaves:

1. Add explicit input/output/inout direction metadata to module ports without
   changing the existing value-oriented module test API.
2. Let each hardware backend lower an inout leaf through its vendor IO-buffer
   primitive or a portable `assign pin = oe ? data : 1'bz` implementation.
3. Add host-side four-state resolution for undriven, driven, and conflicting
   external values. Ordinary NAND wires remain two-state.
4. Extend board bindings and pin-conflict checks to inout ports, then add bank,
   voltage, differential-pair, and dedicated-pin constraints as target data
   requires them.

Until those pieces exist, target bindings must use separate input and output
ports. This prevents a partial abstraction from silently modeling contention
incorrectly.
