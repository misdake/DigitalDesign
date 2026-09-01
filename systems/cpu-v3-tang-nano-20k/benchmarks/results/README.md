# CPU V3 benchmark results

These CSV files were generated from the same 13-program directory in
`../programs`. Each Stage was checked out at its merge commit and run through
`../run-suite.ps1` in release mode. Lower cycle counts are better.

| Stage | Merge commit | Rows |
| --- | --- | ---: |
| 7 | `c56bc92` | 13 |
| 8 | `68b5f88` | 13 |
| 9 | `cf6aef6` | 13 |
| 10 | `76d5bef` | 13 |

`streaming-mix.rs` uses set-shifted bases (`0`, `4112`, and `8240`) rather
than three exactly conflicting 4096-word bases.
