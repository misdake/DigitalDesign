# Production column board build

`build.ps1 -Mode Build -OutputDirectory target/column-board/FRESH` exports and
fits the single production column/POT + fixed400x240 RTT composition at54/108MHz
CL3. `-BootExample` selects firmware, not a second hardware layout. Export/Build
requires a fresh directory; Audit checks the existing generated artifacts,
resource limits and setup/hold timing. Receipts bind source and artifact hashes.

The default CPU Display diagnostic uses the same column allocation and addresses.
A renderer/RTT image must bind matching boot assets. This script is offline and
does not program the board, open UART or capture images.
