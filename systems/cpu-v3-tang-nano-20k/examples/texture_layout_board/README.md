# Pure texture/framebuffer layout board qualification

Run `build.ps1 -Mode Build -Profile texture16x8-column16 -Implementation production
-OutputDirectory target/texture-column16-pure/FRESH` (on one PowerShell line).
The matched row profile is `texture16x8-row`; repeat production builds in separate fresh directories. Both use the same54/108MHz CL3 board constraints.

`production` retains existing place1/route2. Two attempted timing-driven exports
failed before PNR because the library rejects duplicate existing process options;
those logs remain as tooling failures, not timing measurements. No shared tool
library was changed. The bounded comparison therefore repeats the same production
configuration for both layouts, preserving every outcome. Default production hardware is unaffected. No RTT view
is selected. Each Export/Build requires a new target directory; Audit checks
existing generated artifacts and the normal resource/timing requirements.
The receipt records profile, implementation, source HEAD/dirty and report hashes;
the generated Gowin manifest identifies the actual RTL/constraints/boot source.

The default boot is a CPU Display diagnostic used only for complete-board fit.
It is not a qualified column scene firmware. Device programming requires matched
layout firmware/static assets, functional audit, timing pass, and the parent's
exclusive hardware handoff. This script never programs, opens UART, or captures.
