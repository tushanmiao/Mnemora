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

Use `current` for an unclassified diagnostic sample when the visible scene has
not been verified. It is intentionally excluded from the release budgets and
must not be reported as idle, Chat, or PDF acceptance data.

## Synthetic chat pressure tests

Synthetic conversations are repeatable UI stress data, not a replacement for
real Release acceptance scenes. The generator writes only files with IDs tracked
in a manifest and leaves existing conversations untouched:

~~~powershell
node .\scripts\memory\seed-synthetic-conversations.mjs `
  --app-data-dir "$env:APPDATA\com.mnemora.app" `
  --profile heavy
~~~

Restart Mnemora, open the generated conversation, and sample it as `chat` only
when the generated scene is intentionally being measured. Available profiles
are `normal`, `heavy`, `max`, and `sidebar`. Remove only generated files with:

~~~powershell
node .\scripts\memory\seed-synthetic-conversations.mjs `
  --app-data-dir "$env:APPDATA\com.mnemora.app" `
  --cleanup
~~~
