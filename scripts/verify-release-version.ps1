param(
  [Parameter(Mandatory = $true)]
  [string]$ExpectedTag
)

$ErrorActionPreference = "Stop"
$packageVersion = (Get-Content package.json -Raw | ConvertFrom-Json).version
$packageLockVersion = if ((Get-Content package-lock.json -Raw) -match '(?m)^\s*"version"\s*:\s*"([^"]+)"') {
  $Matches[1]
} else {
  throw "Cannot read root version from package-lock.json."
}
$expectedVersion = if ($ExpectedTag -match '^v(?<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$') {
  $Matches.version
} else {
  # 手动运行工作流时 ref_name 通常是分支名，版本仍以项目文件为准。
  $packageVersion
}
$tauriVersion = (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
$cargoVersion = if ((Get-Content src-tauri/Cargo.toml -Raw) -match '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') {
  $Matches[1]
} else {
  throw "Cannot read [package].version from src-tauri/Cargo.toml."
}
$cargoLockVersion = if ((Get-Content src-tauri/Cargo.lock -Raw) -match '(?ms)^\[\[package\]\]\s*name\s*=\s*"mnemora"\s*version\s*=\s*"([^"]+)"') {
  $Matches[1]
} else {
  throw "Cannot read mnemora version from src-tauri/Cargo.lock."
}

$versions = @{
  "package.json" = $packageVersion
  "package-lock.json" = $packageLockVersion
  "src-tauri/Cargo.toml" = $cargoVersion
  "src-tauri/Cargo.lock" = $cargoLockVersion
  "src-tauri/tauri.conf.json" = $tauriVersion
}

foreach ($entry in $versions.GetEnumerator()) {
  if ($entry.Value -ne $expectedVersion) {
    throw "$($entry.Key) version $($entry.Value) does not match tag $ExpectedTag."
  }
}

Write-Output "Release versions match $ExpectedTag."
