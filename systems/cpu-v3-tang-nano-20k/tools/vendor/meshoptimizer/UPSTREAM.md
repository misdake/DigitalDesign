# meshoptimizer clusterizer

Official meshoptimizer 1.3.0 JavaScript/WebAssembly clusterizer, MIT licensed.
The bytes match tag `v1.3`; the `.js` module is named `.mjs` here for explicit
Node.js ES-module loading without an npm installation.

- Source: https://github.com/zeux/meshoptimizer/blob/v1.3/js/meshopt_clusterizer.js
- License: https://github.com/zeux/meshoptimizer/blob/v1.3/LICENSE.md
- Clusterizer SHA-256: ab1841427630c3c7be0fa53cf8f0a5c6b8b284618200be9d7762b807b45cec01

Initially promoted from the same pinned module already installed under the
local `.agent/tools/gpu-v2/meshpack` experiment. Builds use this checked-in copy
and never download packages. Keep the upstream module unchanged; update this
record and verify the native producer when upgrading.
