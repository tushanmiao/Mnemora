param(
  [int]$RootPid = 0,
  [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"

function Get-Role {
  param([bool]$IsRoot, [string]$Name, [string]$CommandLine)
  if ($IsRoot) { return "mnemora" }
  $lowerName = $Name.ToLowerInvariant()
  $lowerCommand = $CommandLine.ToLowerInvariant()
  if ($lowerName.Contains("crashpad")) { return "crashpad" }
  if (-not $lowerName.Contains("msedgewebview2")) { return "other" }
  if ($lowerCommand.Contains("--type=renderer")) { return "webview-renderer" }
  if ($lowerCommand.Contains("--type=gpu-process")) { return "webview-gpu" }
  if ($lowerCommand.Contains("network.mojom.networkservice")) { return "webview-network" }
  if ($lowerCommand.Contains("audio.mojom.audioservice")) { return "webview-audio" }
  if ($lowerCommand.Contains("storage.mojom.storageservice")) { return "webview-storage" }
  if ($lowerCommand.Contains("--type=utility")) { return "webview-utility" }
  return "webview-browser"
}

if ($RootPid -le 0) {
  $candidate = Get-Process -Name "mnemora" -ErrorAction SilentlyContinue |
    Sort-Object StartTime -Descending |
    Select-Object -First 1
  if (-not $candidate) { throw "No mnemora process was found. Pass -RootPid explicitly." }
  $RootPid = $candidate.Id
}

$cimProcesses = @(Get-CimInstance Win32_Process)
$ids = [System.Collections.Generic.HashSet[int]]::new()
[void]$ids.Add($RootPid)
$changed = $true
while ($changed) {
  $changed = $false
  foreach ($process in $cimProcesses) {
    if ($ids.Contains([int]$process.ParentProcessId) -and $ids.Add([int]$process.ProcessId)) {
      $changed = $true
    }
  }
}

$samples = foreach ($process in $cimProcesses | Where-Object { $ids.Contains([int]$_.ProcessId) }) {
  $runtime = Get-Process -Id ([int]$process.ProcessId) -ErrorAction SilentlyContinue
  if (-not $runtime) { continue }
  $name = [string]$process.Name
  $commandLine = [string]$process.CommandLine
  $role = Get-Role -IsRoot (([int]$process.ProcessId) -eq $RootPid) -Name $name -CommandLine $commandLine
  [PSCustomObject]@{
    pid = [int]$process.ProcessId
    parentPid = [int]$process.ParentProcessId
    role = $role
    name = $name
    workingSetBytes = [int64]$runtime.WorkingSet64
    privateBytes = [int64]$runtime.PrivateMemorySize64
  }
}

$result = [PSCustomObject]@{
  capturedAtMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  rootPid = $RootPid
  totalWorkingSetBytes = ($samples | Measure-Object -Property workingSetBytes -Sum).Sum
  totalPrivateBytes = ($samples | Measure-Object -Property privateBytes -Sum).Sum
  processes = @($samples | Sort-Object pid)
}

$json = $result | ConvertTo-Json -Depth 5
if ($OutputPath) {
  $resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputPath)
  $parent = Split-Path -Parent $resolvedOutputPath
  if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw "Output directory does not exist: $parent" }
  [System.IO.File]::WriteAllText($resolvedOutputPath, $json, [System.Text.UTF8Encoding]::new($false))
}
$json
