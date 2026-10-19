$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot 'build-windows-support.ps1')
& python (Join-Path $PSScriptRoot 'check-windows-dependencies.py') (Join-Path $projectRoot 'target\release\svbb.exe') (Join-Path $projectRoot 'runtime\vulkan-1.dll')
if ($LASTEXITCODE -ne 0) { throw 'Windows dependency audit failed' }
$version = ([regex]::Match((Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"')).Groups[1].Value
if (!$version) { throw 'Package version missing' }
$packageRoot = Join-Path $projectRoot "artifacts\SVBB-$version-windows-x64-first-run"
$archive = "$packageRoot.zip"
if ((Test-Path -LiteralPath $packageRoot) -or (Test-Path -LiteralPath $archive)) { throw "Release output already exists: $packageRoot. Move or clean that generated package before rebuilding." }
New-Item -ItemType Directory $packageRoot,(Join-Path $packageRoot 'licenses'),(Join-Path $packageRoot 'source') | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\svbb.exe') -Destination (Join-Path $packageRoot 'SonyVegasButBetter.exe')
Copy-Item -LiteralPath (Join-Path $projectRoot 'runtime\vulkan-1.dll') -Destination $packageRoot
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'start-windows.cmd') -Destination (Join-Path $packageRoot 'Start SVBB.cmd')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'setup-release-runtime.ps1') -Destination (Join-Path $packageRoot 'setup-runtime.ps1')
Get-ChildItem -LiteralPath $projectRoot -File | Where-Object { $_.Extension -eq '.md' -or $_.Name -in @('LICENSE','runtime-manifest.json') } | Copy-Item -Destination $packageRoot
Copy-Item -LiteralPath (Join-Path $projectRoot 'licenses\dependencies') -Destination (Join-Path $packageRoot 'licenses') -Recurse
Copy-Item -LiteralPath (Join-Path $projectRoot 'licenses\cargo-dependencies.tsv') -Destination (Join-Path $packageRoot 'licenses')
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'runtime-manifest.json') -Raw | ConvertFrom-Json
foreach ($item in $manifest.vulkan_loader.source_archives) {
    Copy-Item -LiteralPath (Join-Path $projectRoot ('runtime\' + $item.file)) -Destination (Join-Path $packageRoot 'source')
    $source = Join-Path (Join-Path $projectRoot 'runtime\vulkan-source') $item.root
    $notices = Join-Path (Join-Path $packageRoot 'licenses') $item.root
    New-Item -ItemType Directory $notices | Out-Null
    Get-ChildItem -LiteralPath $source -File -Filter 'LICENSE*' | Copy-Item -Destination $notices
    Copy-Item -LiteralPath (Join-Path $source 'LICENSES') -Destination $notices -Recurse
}
@'
SVBB for Windows 10/11 x64

1. Extract the ENTIRE ZIP into a writable folder (for example Documents\SVBB).
2. Double-click "Start SVBB.cmd".
3. First launch downloads and verifies about 250 MB of media tools from their
   upstream GitHub releases. No administrator access, Rust, or 7-Zip is required.
4. Later launches work offline. Keep the runtime folder beside the editor.

This is an early preview, unsigned. If Windows blocks it, use your own judgement;
the launcher does not change Windows security settings. Do not run inside the ZIP.
If setup fails, read setup.log and run the launcher again. Verified downloads can
be reused after an interrupted setup. The launcher also repairs altered runtime files.

The public ZIP contains the editor, notices, source and setup scripts. The FFmpeg
and libmpv binaries are fetched directly from upstream rather than redistributed
inside this archive. Their versions, URLs and hashes are in runtime-manifest.json.
The app-local Vulkan loader is included with its source and notices. The editor
and loader use the static MSVC runtime; no Visual C++ runtime installer is needed.

See PLAYTEST.md for shortcuts, README.md for features, and TESTING.md for coverage.
'@ | Set-Content -LiteralPath (Join-Path $packageRoot 'START HERE.txt') -Encoding UTF8
& git -C $projectRoot archive --format=zip -o (Join-Path $packageRoot 'source\svbb-source.zip') HEAD
if ($LASTEXITCODE -ne 0) { throw 'Source archive failed' }
Get-ChildItem -LiteralPath $packageRoot -Recurse -File | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),$_.FullName.Substring($packageRoot.Length + 1)
} | Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt') -Encoding UTF8
Compress-Archive -LiteralPath (Get-ChildItem -LiteralPath $packageRoot | Select-Object -ExpandProperty FullName) -DestinationPath $archive -CompressionLevel Optimal
Write-Output $archive
