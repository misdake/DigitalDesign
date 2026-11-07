# External Flash and cold-start workflow

| Flash byte range | Owner |
| --- | --- |
| [0,0x100000) | FPGA configuration reserve |
| [0x100000,0x100004) | CB06 signature |
| [0x100004,image end) | Sequential COPY/ZERO commands, inline COPY data and END |

The [image encoding](boot-image-format.md) has one authority. The total image
must fit the 8-MiB device. ZERO regions and DRAM address holes occupy no payload.

The [application command](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md)
selects an owned recipe; the default recipe reads boot-applications.conf.
The build fixes resources, generates boot_resources.rs, compiles one application,
checks host ownership and packs the stream. OUT_DIR is the sole generated-byte
authority; cpu-v3-boot-assets exports the code, package, linked metadata and
bound source/resource snapshot into a fresh directory.

Reconstruct application.v3bin with cpu-v3-pack --project followed by the
project configuration path, and compare bytes. The packer reads the adjacent
application.static-words; externally compiled binaries require that metadata
or an explicit --static-words N. The optional --configuration-bin
and --flash-image arguments create the complete persistent image: configuration
at 0, package at 0x100000, and an erased-value gap. A configuration exceeding its
1-MiB reserve is rejected. The board script passes the selected project file so
resource COPY data cannot silently disappear from its reconstruction. The tool
belongs to cpu-v3-boot-format and does not trigger an application build. It uses
the build script's encoder, so agreement qualifies inputs rather than an
independent encoding algorithm.

V5 and CB06 images are incompatible. The first CB06 deployment must pair new
gateware and a CB06 image. Later application/resource updates can program only
the package at 0x100000 while retaining the same format/layout gateware.
Configuration or power cycling runs boot again; buttons and software do not.
Packages omitting framebuffer ZERO require the matching virtual-black-start
gateware; do not install them alone over gateware that assumes cleared A/B.
Existing CB06 streams with explicit ZERO commands remain accepted.

Cold PLL/SDRAM readiness starts the loader. CPU and GPU remain quiet during
COPY/ZERO, and Display emits black timing without framebuffer requests.
Only the allocated static prefix is zeroed; the unused static tail is untouched. Code/resources
reach their final locations, and END plus all actual write responses produces
load_done. Display then publishes black group markers without touching DRAM
and acknowledges black timing at a frame origin. After cache cold-ready,
boot_done releases the CPU. A/B and Depth remain untouched. The first completed
software present activates real framebuffer reads and the first two complete
groups enable real video at a frame origin. Clock loss requires cold
reconfiguration, rather than a partial automatic reset.

The default build preserves nominal 13.5-MHz single-line reading. Setting
CPU_V3_SPI_HALF_PERIOD=1 builds the nominal 27-MHz candidate; retain separate
output directories and source/image hashes when comparing them. An offline
build or protocol simulation is not physical Flash qualification.

Boot owns LEDs until release, using the failure codes in the format document.
Afterward the selected [Display diagnostic](system-display-test.md) owns LEDs
and UART, reports DDHT 0x0b after actual target switches and keeps the GPU idle.
Programming, UART observation and cold-power verification require their own
authorized physical procedure; offline preparation does not perform them.

## Frozen hardware and deployment

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin cpu-v3-system -- target/hardware --build
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py audit target/hardware
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py assemble target/hardware target/application --out target/deployment
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py verify target/hardware target/deployment
```

The normal board binary contains no application producer. Synthesis source
generation checks HDL signatures, dependencies and resource ownership without
constructing simulation testbenches; board qualification remains in the original
example's explicit tests. The board namespace and generated HDL identities stay
the same. [Frozen artifact audit](/hardware/vendor/gowin/doc/hardware-artifacts.md)
loads existing sources and fit reports without exporting today's board.

Assembly requires the complete `cpu_v3_system` / `CpuV3System` hardware identity
and a verified same-build application artifact. It derives raw configuration
bytes directly from the audited FS, validates the configuration reserve and
fitted Flash capacity, then writes configuration, application-only and complete
images into a fresh directory. `deployment.json`, published last, binds all file
hashes, the hardware descriptor and application artifact. Verification repeats
the hardware audit and exact Flash composition. Moving an unchanged hardware
directory does not change its identity.

Only explicit programming subcommands open a device:

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py program-flash target/hardware target/deployment --kind application --receipt target/application-program.json
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py program-flash target/hardware target/deployment --kind complete --receipt target/complete-program.json
python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py program-sram target/hardware target/deployment --receipt target/sram-program.json
```

The System selects offsets from the image kind, never a user-supplied number.
Each command revalidates the bundle/hardware and records payload identity,
offset, tool invocation and actual success/failure in a fresh receipt.
`--runtime` can pin the standalone vendor consumer; otherwise only that normal
vendor binary is built, with a 600-second build limit. Each audit is bounded to
60 seconds and programming to 600 seconds. No programming runs during assembly,
verification or report generation. Compatibility with the current CB06 and
virtual-black-start gateware remains the caller's deployment requirement.


## Explicit GPU API application selection

Format-2 project files may declare `gpu-api` to select the host-approved explicit
Gpu/CommandBuffer root and checked RCC frontend. The retired `renderer` directive
is rejected. The policy is not inferred from application imports, and duplicate
directives are rejected. These are compilation
policies only: they emit identical resource constants and do not change CB06.
The `gpu_renderer` example selector sets `gpu-api`; its generated repack project
uses the same directive. Other named legacy examples retain their existing root.
