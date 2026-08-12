# Hardware development scripts

This directory contains host-side tools shared by multiple hardware modules
and board examples. Example-specific RTL and test vectors remain under
`hardware/examples/`; reusable capture, decode, build, and reporting tools
belong here.

`check_uart_status.ps1` validates a raw UART capture by requiring repeated
success bytes and rejecting any failure byte. It defaults to ASCII `P`/`F`,
but another hardware self-test can select different byte values:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/scripts/check_uart_status.ps1 `
    -Path target/example/capture.bin `
    -SuccessByte 0x50 -FailureByte 0x46 -MinimumSuccessBytes 3
```

This byte-status protocol is intentionally small. Tests that need error
addresses, observed values, or replayable vectors should use a framed protocol
and add a corresponding general decoder here rather than growing a private
script inside one example.
