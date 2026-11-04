<#
.SYNOPSIS
  Builds the plugin component (dist\plugin.wasm).
.PARAMETER Pack
  Also writes the installable package dist\<id>-<version>.oiplugin.
.PARAMETER Link
  Links this folder into OpenIDEA (Settings > Plugins > Reload to load a new build).
.PARAMETER Unlink
  Removes the link.
#>
param([switch]$Pack, [switch]$Link, [switch]$Unlink)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not ((rustup target list --installed) -contains 'wasm32-wasip2')) { rustup target add wasm32-wasip2 }
# Through cmd: Windows PowerShell turns a native command's stderr (cargo's progress) into errors.
cmd /c "cargo build --release --locked --target wasm32-wasip2 2>&1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\wasm32-wasip2\release\openidea_cfxlua.wasm dist\plugin.wasm -Force
Write-Host "dist\plugin.wasm ($((Get-Item dist\plugin.wasm).Length) bytes)"

$manifest = Get-Content plugin.toml -Raw
$id = [regex]::Match($manifest, '(?m)^id\s*=\s*"([^"]+)"').Groups[1].Value
$version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value

if ($Pack) {
  $stage = Join-Path ([IO.Path]::GetTempPath()) ("cfxlua-pack-" + [guid]::NewGuid())
  New-Item -ItemType Directory -Force (Join-Path $stage 'dist') | Out-Null
  Copy-Item plugin.toml, settings.schema.json, LICENSE, NOTICE $stage
  Copy-Item dist\plugin.wasm (Join-Path $stage 'dist')
  foreach ($d in 'icons', 'templates', 'web') { if (Test-Path $d) { Copy-Item $d $stage -Recurse } }
  # Entries named with forward slashes (Compress-Archive on Windows PowerShell writes backslashes).
  Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
  $out = Join-Path $root "dist\$id-$version.oiplugin"
  if (Test-Path $out) { Remove-Item $out -Force }
  $archive = [IO.Compression.ZipFile]::Open($out, [IO.Compression.ZipArchiveMode]::Create)
  try {
    Get-ChildItem $stage -Recurse -File | ForEach-Object {
      $name = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
      [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $_.FullName, $name, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
  } finally { $archive.Dispose() }
  Remove-Item $stage -Recurse -Force
  Write-Host $out
}

if ($Link -or $Unlink) {
  $plugins = if ($env:OPENIDEA_PLUGINS_DIR) { $env:OPENIDEA_PLUGINS_DIR } else { Join-Path $env:APPDATA 'ai.l0ng.openidea\plugins' }
  New-Item -ItemType Directory -Force $plugins | Out-Null
  $file = Join-Path $plugins 'links.json'
  $paths = @()
  if (Test-Path $file) { $paths = @((Get-Content $file -Raw | ConvertFrom-Json).paths) }
  $paths = @($paths | Where-Object { $_ -and ($_ -ne $root) })
  if ($Link) { $paths += $root }
  $json = ConvertTo-Json -InputObject @{ paths = @($paths) } -Depth 3
  [IO.File]::WriteAllText($file, $json)
  Write-Host ($(if ($Link) { "linked $root" } else { "unlinked $root" }) + " - open Settings > Plugins in OpenIDEA and press Reload")
}
