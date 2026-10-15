# Historical geometry references

Two immutable standalone reference implementations survive the experiment cleanup:
`interpolator_ii4.v` is the selected decoded-Vertex/result-cache/Setup/raster/
interpolation comparison point; `setup_addressmin.v` includes local input/result
storage and clipping/Setup but excludes raster and pixel shading.

These are reference fixtures, not selected production factories. They retain
their old precision and input boundaries. They do not implement the current
canonical V96/T64/full-drain contract or establish complete GPU fit/board proof.
The external frontend rewrite remains independent.

Run `python ip/gpu-v2/examples/geometry_reference/replay.py CASE OUTPUT` with
a fresh output directory under `target/`. The five names and finite timeouts
are listed in `cases.json`. Set `GOWIN_HOME`, and either configure
`IVERILOG_EXE`/`VVP_EXE` or put them on PATH. Inputs and source bytes are checked
against the immutable fixture manifest before execution. The captured gold is
historical oracle evidence; these replay tests do not replace a new numerical
accuracy or integration qualification.

See [the experiment review](../../docs/experiment-review.md) for disposition.
