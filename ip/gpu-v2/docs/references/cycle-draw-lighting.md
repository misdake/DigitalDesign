# Selected live-global Lighting cycles

`vliw_cycle::lighting::Lighting` models the command profile's simultaneous
Normal/NDC tuple adapter and selected full H9 core: II4, arithmetic span 34,
nine wide multiplication lanes and shared normalization ROM ports. It uses
the same frozen instruction schedule as the actual factory. Each instruction
executes at its issue age from then-available operands and commits only at its
resource ready age. It never computes a whole pixel answer at admission.

The model retains numerical values by instruction identity for host execution;
these host vectors are not an inventory of physical RTL registers. Its public
observation covers valid numerical results, the exact valid/calendar state,
live epoch and physical ID taps. Invalid free-running arithmetic payloads are
outside that numerical observation. The RTL qualifier projects those payloads
to unknown explicitly; it does not claim their raw DSP bits were compared.

## Source and return ownership

The producer pool holds its paid Normal Q and NDC XY through tuple acceptance.
Three signed 12-bit normal components expand by wiring to S16F14; the input rows
pack normal XY, normal Z/NDC X, then NDC Y. The selected input-row registers and
modulo retiming are represented by the accepted pixel's scheduled source reads.
The normal loan is already present before phase-zero admission; this is the
closed pool tuple contract, rather than an arbitrary changing input stream.

Only covered lanes enter the arithmetic pipe. Masked lanes may ACK independently
of core phase. The token packs quad slot, lane and last-covered-lane status.
ID storage captures its head every enabled edge and shifts the nine later taps
on old phase1, retaining payload through reset. A 35-bit valid pipe supplies the
return gate. Admission inserts bit0; 34 following advancing edges expose bit34.
Thus the pre-edge test counter records a distance of 35 enabled clock calls
between admission observation and first return observation.

Compute CE controls phase, validity, arithmetic and ID advancement. CE0 preserves
them. `out_ready` does not independently stop this live-global core; the command
parent owns compute_tick. Reset clears phase and validity, without resetting
unowned numerical/ID payload. No extra output queue or same-edge credit exists.

## Live globals and composition

Light, projection, intensities, exponent code and epoch are live wires, without
a context-install handshake or per-pixel context snapshot. Instruction execution
reevaluates context-only wire cones on the current enabled edge. Registered
operation results retain their ordinary schedule lifetime. Epoch is live even
under CE0. Output mode1 substitutes current ambient and zero H; mode2 masks H;
the full schedule remains selected in all modes.

`Renderer::with_lighting(profile)` connects the actual pool tuple, live globals
and return bank while retaining explicit Sampling ports. The lower
`with_triangles` constructor preserves external numerical peers for isolated
composition. `draw::Gpu::with_draws` selects real Lighting and supplies installed
CP globals; it also closes the selected Sampling owner. Each producer, backend
and shader owner clocks once from the same old-state wiring, including when
another owner reports an error.

## Qualification boundary

`vliw_cycle_lighting` checks masked lanes, II4 admission, stalls, warm reset,
physical ID taps, live epoch/mode, ignored output-ready and an ambient change
after admission. An independent quantized stage oracle plus independently
regenerated mathematical H9 endpoints checks the numerical result. Portable and
vendor hard-DSP production RTL use the same bounded inputs. Compiled mutations
exercise epoch wiring, ready/CE confusion and the H9 result bit.

The two-buffer RGB/lit native fixture drives actual CP, DMA, pool, Lighting,
Final and framebuffer maintenance, checks all backing words after END, and
compares an actual CP-to-router RTL root with native-MC feedback. That fixture
uses ambient output mode; nonzero full-H9 arithmetic is separately covered by
the tuple fixture. The separate
[UV/lit native and whole command-root fixtures](/ip/gpu-v2/docs/references/cycle-draw-pixel.md)
exercise nonidentity light and nonzero H9 with actual Sampling, Final and
depth/Q6 output. Whole-system/board qualification remains outside these results.
