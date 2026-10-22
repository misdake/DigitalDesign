# External Flash and cold-start workflow

| Flash byte range | Owner |
| --- | --- |
| [0,0x100000) | FPGA configuration reserve |
| [0x100000,0x100004) | CB06 signature |
| [0x100004,image end) | Sequential COPY/ZERO commands, inline COPY data and END |

The [image encoding](boot-image-format.md) has one authority. The total image
must fit the 8-MiB device. ZERO regions and DRAM address holes occupy no payload.

boot-applications.conf selects one RCC application and optional named resources.
The build fixes resource addresses, generates boot_resources.rs, compiles the
application, checks host layout ownership and packs the stream. OUT_DIR is the
sole generated-byte authority; cpu-v3-boot-assets exports its four files.

Independently repack application.v3bin with cpu-v3-pack --project followed by the
project configuration path, and compare bytes. The optional --configuration-bin
and --flash-image arguments create the complete persistent image: configuration
at 0, package at 0x100000, and an erased-value gap. A configuration exceeding its
1-MiB reserve is rejected. The board script passes the selected project file so
resource COPY data cannot silently disappear from its independent repack.

V5 and CB06 images are incompatible. The first CB06 deployment must pair new
gateware and a CB06 image. Later application/resource updates can program only
the package at 0x100000 while retaining the same format/layout gateware.
Configuration or power cycling runs boot again; buttons and software do not.

Cold PLL/SDRAM readiness starts the loader. CPU and GPU remain quiet during
COPY/ZERO, and Display emits black timing without framebuffer requests.
The static arena and both full framebuffer slots are zeroed, code/resources
reach their final locations, and END plus all actual write responses produces
load_done. Display then prefetches both initial groups and acknowledges a new
frame beginning from A. After cache cold-ready, boot_done releases the CPU.
Depth remains untouched until GPU Pass Clear. Clock loss requires cold
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
