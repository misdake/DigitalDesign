# Tank Racer camera playback

`bench-tankracer-static` accepts two forms of `--camera` JSON. A list of views
advances one entry per submitted frame, preserving the measurement workflow.
Each view contains `eye`, `target` and optional `up` in source-world coordinates.

A `tankracer.camera-loop.v1` object provides `views` as closed path knots,
`duration_seconds` (1–60) and `frames_per_second` (1–60). The build expands the
path into equally spaced samples by cumulative eye-position distance. It linearly
interpolates position and normalized forward direction, rebuilds each view basis
and fixes `up` to world Y. The last knot connects to the first without duplicating
the endpoint. At most 2730 expanded views fit the u16 camera asset-window offset.

`assets/england/cameras-loop.json` selects one complete recorded lap, with a
shared seam pose, 30 samples/s and a 60-second period. Its 1800 views occupy
86,400 immutable asset bytes; only the current 48-byte view is read by the CPU.
The public GPU recording and Prepared ring paths are unchanged.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/camera_path.py systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras.json systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/route.json systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras-loop.json
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --camera systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras-loop.json --recording prepared --out target/tankracer-loop
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_camera_path.py
```

`Playback` starts after scene/Prepared initialization. It maps elapsed system
milliseconds to a camera sample, repeating or skipping samples as rendering
speed changes. It rebases the loop epoch before the independent 16-bit timer
wrap; calls must remain less than 65,536 ms apart. This applies to ordinary
recording, Prepared submission and the large-tail fallback. The nominal loop
period is independent of frame rate; visible presentation includes pipeline
latency and vblank quantization. Functional previews need host time advancement
to move a timed camera; ordinary lists remain suitable for indexed probes.
