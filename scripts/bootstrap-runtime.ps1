$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$runtimeDir = Join-Path $projectRoot 'runtime'
New-Item -ItemType Directory -Force $runtimeDir | Out-Null
$manifest = Get-Content (Join-Path $projectRoot 'runtime-manifest.json') -Raw | ConvertFrom-Json
foreach ($item in $manifest.archives) {
    $archivePath = Join-Path $runtimeDir $item.file
    if (!(Test-Path -LiteralPath $archivePath)) { Invoke-WebRequest $item.url -OutFile $archivePath }
    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $item.sha256) { throw "Checksum mismatch for $($item.file). Refusing changed runtime." }
}
Expand-Archive -LiteralPath (Join-Path $runtimeDir 'essentials.zip') -DestinationPath (Join-Path $runtimeDir 'essentials') -Force
$ffBin = Join-Path $runtimeDir 'essentials\ffmpeg-9.0.2-essentials_build\bin'
Copy-Item -LiteralPath (Join-Path $ffBin 'ffmpeg.exe') -Destination $runtimeDir
Copy-Item -LiteralPath (Join-Path $ffBin 'ffprobe.exe') -Destination $runtimeDir
$sevenZip = Get-Command 7z.exe -ErrorAction SilentlyContinue
if ($sevenZip) { $extractor = $sevenZip.Source } else { $extractor = 'C:\Program Files\7-Zip\7z.exe' }
if (!(Test-Path -LiteralPath $extractor)) { throw '7-Zip is required to extract libmpv. Install it or add 7z.exe to PATH.' }
& $extractor x (Join-Path $runtimeDir 'mpv.7z') "-o$(Join-Path $runtimeDir 'mpv')" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'libmpv extraction failed' }
foreach ($item in $manifest.binaries) {
    $actualHash = (Get-FileHash -LiteralPath (Join-Path $runtimeDir $item.path) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $item.sha256) { throw "Binary checksum mismatch: $($item.path)" }
}
Write-Output 'Pinned media runtime ready.'
