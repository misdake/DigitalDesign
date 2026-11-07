# Script contracts

- `check-layering.ps1` exercises the real workspace checker with in-memory Cargo
  metadata: same-system support, forbidden reverse/cross-system dependencies,
  frontend/CPU direction and independent modeling boundaries. It creates no
  checkout, package or build output.

Run from the repository root:

```powershell
& scripts/tests/check-layering.ps1
```
