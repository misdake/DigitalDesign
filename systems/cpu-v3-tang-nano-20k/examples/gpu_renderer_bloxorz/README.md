# Bloxorz RTT presentation

This continuous CPU V3 application uses the existing two integer-rule fixtures,
ordinary GPU meshes and the resource renderer. Restart, fall restart and level
completion capture their current scene and fade through black into a fresh
snapshot of the destination. Entry and goal animations run separately from the
fade. Moves and connection status remain in the captured HUD.

RIGHT/LEFT/DOWN/UP start a roll; START restarts and TRIANGLE changes fixture.
All keys are ignored during a fade. Ordinary animation locks movement; restart
can interrupt it. Disconnected input is ignored while presentation continues. Successful
completion advances to the next fixture after the goal animation.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/build.ps1
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-rtt -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','host','gpu_pipeline::bloxorz::')
```

The explicit `gpu_renderer_bloxorz` selector pins column layout and RTT command
ABI2, checks actual RCC library bodies with rustc, and compares generated boot
bytes against independent CB06 packaging. Products stay under this worktree's
`target/`. This does not change the default boot application or program a board;
matching `production` GPU and Display hardware is required.

See the [presentation documents](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/docs/README.md) for phase ordering, snapshot ownership, RGB arithmetic and verification.

This remains a two-fixture application. The split fixture keeps its existing
whole-block display limitation. Full original levels, undo, split rendering,
shadows and the reference's detailed tipping physics are outside this entry.

For unattended capture-card diagnosis, build with `-AutomaticCapture`. Add
`-CaptureBrightness 255` for the full-brightness comparison; the default is128. This
explicit selector runs entry, holds PLAY for2s of clamped application time,
makes one real +X move, holds the result for2s, then restarts and halts only
after the selected RTT brightness consumer has completed WAIT_ALL. Display keeps scanning
the last framebuffer. It ignores physical input and does not invoke the PC
profiler. UART progress is ASCII: stage plus four hexadecimal digits and LF;
A=loop number, B=source begin, C=source retired, D=consumer submit,
E=consumer wait, F=presented mode, H=selected-brightness halt. This diagnostic does not
change the ordinary application selector.

The initial fixture contains a switch at(2,1), a bridge at(5,1) and its goal
at(6,1). The first +X move presses that switch, so the bridge appears next to
the goal. These are test fixtures, not the Web game's first two levels.

Application Flash writes do not install the matching FPGA configuration. Load
an audited `production` SRAM image after writing the application; cold
power-on additionally needs that configuration in Flash. A previous stored
FPGA profile cannot qualify this RTT application. The capture diagnostic draws
`1000 + tint` over the consumer (1128 or1255 at the stop) and reports active Display
address/format/pending and GPU status after its selected-brightness halt marker.

`1128`, the halt marker and idle/completion status indicate software/protocol
progress only. They do not validate source pixels, texture addressing, color
results or HDMI output; corrupted frames can carry the same overlay. Bind UART
and pictures to the same programming interval before interpreting them together.

To compose a persistent pair, pass `-HardwareArtifactDirectory target/.../board`
to this recipe (optionally with `-AutomaticCapture`). It audits the existing
RTT fit against current sources, checks that the raw configuration binary is
byte-for-byte the audited FS bitstream, and uses the standard packer to produce
`complete-flash.bin`: FPGA configuration at0 and the same independently checked
CB06 application/resources at0x100000. `profile.json` binds all three hashes.
The recipe remains offline. Application-only output is suitable for later
updates only when the stored FPGA configuration already supports this profile;
an application write can return the board to its stored FPGA configuration.
