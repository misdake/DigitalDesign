# Hardware development scripts

This directory contains host-side tools shared by multiple hardware modules
and board examples. Example-specific RTL and test vectors remain under
`hardware/examples/`; reusable capture, decode, build, and reporting tools
belong here.

`check_uart_status.ps1` validates a raw UART capture containing repeated
eight-byte status frames. A frame contains the `DDHT` magic, protocol version,
test ID, result, and XOR checksum. The checker rejects stale captures, frames
for another test, bad checksums, and any reported failure:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/scripts/check_uart_status.ps1 `
    -Path target/example/capture.bin `
    -TestId 0x01 -MinimumSuccessFrames 2
```

Status `0` is success and every other status is failure. Tests that need error
addresses, observed values, or replayable vectors should introduce a new
protocol version and extend the shared decoder rather than growing a private
script inside one example. Set `-MaximumAgeSeconds 0` only when deliberately
inspecting an archived capture.

Assigned test IDs:

| ID | Test |
| ---: | --- |
| `0x03` | Tang Nano 20K fitted SDRAM burst/refresh self-test |
