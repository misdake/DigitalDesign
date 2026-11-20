# CPU V3 application development

This system runs one RCC application on the Tang Nano 20K, with DS2 input,
GPU v2, SDRAM and an HDMI framebuffer. Start by building and simulating an
application; FPGA synthesis and the full hardware test suite are separate work.

1. [Application quickstart](/systems/cpu-v3-tang-nano-20k/docs/application-quickstart.md):
   prerequisites, the first build/frame, input and creating an application.
2. [First GPU application](/systems/cpu-v3-tang-nano-20k/examples/sample_hello_gpu/README.md):
   a small triangle/UI application and copyable recipe; then explore the
   [system showcase](/systems/cpu-v3-tang-nano-20k/examples/demo_showcase/README.md).
3. [Build and artifact workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md):
   application recipes, immutable snapshots and packing existing inputs.
4. [System reference](/systems/cpu-v3-tang-nano-20k/docs/README.md):
   API/storage, devices, boot, profiling and hardware specifications.

The target language is a documented Rust subset, not unrestricted Rust.
Consult the [RCC guide](/compiler/rcc/src/frontend/spec.md) when a language construct is
rejected. CPU V2 commands and retained legacy renderer fixtures are not the
normal CPU V3 application path.
