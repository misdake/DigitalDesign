# CPU V3 system boot format

This system-owned support package contains the std-only CB06 format, project
reader, functional installer and existing-input `cpu-v3-pack` command. Normal
builds have no workspace or external dependencies. RCC/CPU dev dependencies
only preserve the original CLI qualification's shared fixture.

The [CB06 specification](/systems/cpu-v3-tang-nano-20k/docs/boot-image-format.md)
and [application workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md)
own the format and command contracts. The complete System re-exports the library
API; its build script uses the same encoder. No application bytes are generated
by this package.
