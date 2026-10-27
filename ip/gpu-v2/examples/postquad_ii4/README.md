# Reproducible II4 post-interpolator qualification

Export the current `gpu-v2-postquad-export OUTPUT selected [portable]` factory,
then run `python ip/gpu-v2/examples/postquad_ii4/replay.py SOURCE OUTPUT CASE`.
Use a fresh output directory under `target/`. Cases are `hot`, `hot-stall`,
`cold-stall`, `rich`, `paid-bad` and `paid-abort`. Each run has an HDL edge bound
and a process timeout. `IVERILOG_EXE` and `VVP_EXE` override PATH tools;
native simulation also requires `GOWIN_HOME`.

The 400x240 fixture drives 384 quads/1536 pixels at lower-right tile origins.
Healthy runs compare every RGB, ordered commit and all framebuffer bytes with
the independent integer oracle. Cold runs replace nine tiles and pay real memory
beats; faults check immediate cancellation and draining accepted transactions.
Private-core contract monitors are included in the fault fixtures. These tests
do not qualify upstream geometry, CPU integration or a physical board.

`python ip/gpu-v2/examples/postquad_ii4/fit.py SOURCE OUTPUT 0` rebuilds the
registered complete-chain wrapper at 54 MHz. Use placement `1` for its matched
second layout. Configure `GOWIN_HOME` or `GW_SH_EXE`; this tool never programs
a board. The wrapper's 407 registers are included in resource totals.

The fixtures were extracted from independently replayed experiments. Their
manifest is immutable; changing a fixture requires a new oracle review.
The selected VLIW pool and backend contract is in
[pixel-system](/ip/gpu-v2/docs/execution.md); this standalone replay keeps its
own external eight-row input boundary.
