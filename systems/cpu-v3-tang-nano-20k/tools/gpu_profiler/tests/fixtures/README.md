# Frozen capture ABI fixture

`gpu_capture_abi4.rs` is the byte-preserved Git LF blob of
`systems/cpu-v3-tang-nano-20k/rcc/gpu_capture_abi.rs` at accepted
`0aacaeb05a95ab7afcccf8d8ba7f26242967b026`.
SHA256: `9f2e20af855e72aa0909f8e87ff004375abe12bc6055a060fe3ba9c30e55a731`.
It tests the public explicit `--abi` compatibility entry; do not update it with
the current ABI. Tests also derive the Windows CRLF form from these exact bytes.

`gpu_capture_abi6.rs` is the byte-preserved Git LF definition from accepted
`079ce055bde5b9b820a1b453812d24457c41ddfb`, SHA256
`7ce237550ad7fd53e372af84a6de36ae0027dae023b885093818d5de653a7afa`.
It exists only to qualify historical import; current execution rejects ABI6.
Neither archived definition is a current producer or numerical implementation.
