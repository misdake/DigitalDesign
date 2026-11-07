# Shared application preparation

`mod.rs` aggregates declarations from each application's `application.rs` and
implements common producer/manifest/resource preparation. The host tools library
compiles this source once; both the command and System build script consume it.
Source/mode/producer choices remain with the application owner. Preparation has
no RCC dependency and runs only when the System explicitly requests a build.

The [application workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md)
owns the current build and snapshot contract.
