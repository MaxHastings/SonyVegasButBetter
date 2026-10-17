$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$packageRoot = Join-Path $projectRoot 'artifacts\SVBB-0.1.0-windows-x64'
New-Item -ItemType Directory -Force $packageRoot,(Join-Path $packageRoot 'runtime'),(Join-Path $packageRoot 'licenses'),(Join-Path $packageRoot 'source') | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\svbb.exe') -Destination (Join-Path $packageRoot 'SonyVegasButBetter.exe')
foreach ($file in @('ffmpeg.exe','ffprobe.exe')) { Copy-Item -LiteralPath (Join-Path $projectRoot "runtime\$file") -Destination (Join-Path $packageRoot 'runtime') }
Copy-Item -LiteralPath (Join-Path $projectRoot 'runtime\mpv\libmpv-2.dll') -Destination (Join-Path $packageRoot 'runtime')
Get-ChildItem -LiteralPath $projectRoot -File | Where-Object { $_.Extension -eq '.md' -or $_.Name -in @('LICENSE','runtime-manifest.json') } | Copy-Item -Destination $packageRoot
Copy-Item -LiteralPath (Join-Path $projectRoot 'runtime\essentials\ffmpeg-9.0.2-essentials_build\LICENSE') -Destination (Join-Path $packageRoot 'licenses\FFmpeg-GPLv3.txt')
Copy-Item -LiteralPath (Join-Path $projectRoot 'runtime\essentials\ffmpeg-9.0.2-essentials_build\README.txt') -Destination (Join-Path $packageRoot 'licenses\FFmpeg-build.txt')
Get-ChildItem -LiteralPath (Join-Path $projectRoot 'licenses') -File | Copy-Item -Destination (Join-Path $packageRoot 'licenses')
Copy-Item -LiteralPath (Join-Path $projectRoot 'licenses\dependencies') -Destination (Join-Path $packageRoot 'licenses') -Recurse -Force
& git -C $projectRoot archive --format=zip -o (Join-Path $packageRoot 'source\svbb-source.zip') HEAD
if ($LASTEXITCODE -ne 0) { throw 'Source archive failed' }
Get-ChildItem -LiteralPath $packageRoot -Recurse -File | ForEach-Object {
    $relativePath = $_.FullName.Substring($packageRoot.Length + 1)
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),$relativePath
} | Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt')
$archive = Join-Path $projectRoot 'artifacts\SVBB-0.1.0-windows-x64.zip'
& 'C:\Program Files\7-Zip\7z.exe' a -tzip -mx=7 $archive "$packageRoot\*" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Packaging archive failed' }
Write-Output $packageRoot
Write-Output $archive
