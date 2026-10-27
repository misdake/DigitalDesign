# System software tools

- gpu_layout.py reads rcc/gpu_layout.rs for asset producer bounds.
  GPU addresses have one authority shared by RCC, Rust boot packing and Python
  asset producers; the reader rejects unsupported expressions and overflow.
