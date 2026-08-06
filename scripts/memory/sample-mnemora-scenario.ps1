param(
  [Parameter(Mandatory = $true)]
  [int]$RootPid,
  [Parameter(Mandatory = $true)]
  [ValidateSet("idle", "chat", "pdf", "english", "settings", "postHeavyResidual", "current")]
  [string]$Scene,
  [int]$Samples = 6,
  [int]$IntervalSeconds = 5,
  [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
if ($Samples -lt 1 -or $Samples -gt 120) { throw "Samples must be between 1 and 120." }
if ($IntervalSeconds -lt 0 -or $IntervalSeconds -gt 3600) { throw "IntervalSeconds must be between 0 and 3600." }

$sampler = Join-Path $PSScriptRoot "sample-mnemora-process-tree.ps1"
$samplesOut = [System.Collections.Generic.List[object]]::new()
for ($index = 0; $index -lt $Samples; $index++) {
  $process = (& $sampler -RootPid $RootPid | ConvertFrom-Json)
  $samplesOut.Add([PSCustomObject]@{
      scene = $Scene
      process = $process
      page = $null
      sampleIndex = $index
  })
  if ($index + 1 -lt $Samples -and $IntervalSeconds -gt 0) {
    Start-Sleep -Seconds $IntervalSeconds
  }
}

$report = [PSCustomObject]@{
  schemaVersion = 1
  source = "powershell-process-tree"
  capturedAtMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  samples = @($samplesOut)
}
$json = $report | ConvertTo-Json -Depth 8
if ($OutputPath) {
  $resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputPath)
  $parent = Split-Path -Parent $resolvedOutputPath
  if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw "Output directory does not exist: $parent" }
  [System.IO.File]::WriteAllText($resolvedOutputPath, $json, [System.Text.UTF8Encoding]::new($false))
}
$json
