# Mnemora memory benchmarks

The budgets use process-tree `Private Bytes`, in bytes. The mandatory product
gates are the `release` values in `budgets.json`; stretch values are tracked
separately and must never replace a failing mandatory gate.

The PowerShell sampler is read-only and does not start, stop, reload, focus, or
otherwise operate a Mnemora window:

```powershell
.\scripts\memory\sample-mnemora-process-tree.ps1 -RootPid 1234 -OutputPath .\benchmarks\memory\reports\sample.json
node .\scripts\memory\compare-memory-report.mjs .\benchmarks\memory\reports\sample.json .\benchmarks\memory\budgets.json
```

Use a Release instance for acceptance. Development and HMR-soaked samples are
trend data only. Do not commit files under `reports/`.

For a repeatable process-only baseline, keep the app in the requested scene and
run the sampler without interacting with the window:

```powershell
.\scripts\memory\sample-mnemora-scenario.ps1 `
  -RootPid 1234 -Scene idle -Samples 6 -IntervalSeconds 5 `
  -OutputPath .\benchmarks\memory\reports\idle.json
node .\scripts\memory\compare-memory-report.mjs `
  .\benchmarks\memory\reports\idle.json .\benchmarks\memory\budgets.json
```

Capture `postHeavyResidual` only after leaving a heavy scene. The isolation
review threshold is the `postHeavyResidual` budget; it is an investigation
signal, not permission to move a module into another WebView by itself.
