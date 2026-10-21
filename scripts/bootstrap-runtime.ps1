$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$runtimeDir = Join-Path $projectRoot 'runtime'
New-Item -ItemType Directory -Force $runtimeDir | Out-Null
$manifest = Get-Content (Join-Path $projectRoot 'runtime-manifest.json') -Raw | ConvertFrom-Json
foreach ($item in $manifest.archives) {
    $archivePath = Join-Path $runtimeDir $item.file
    if (!(Test-Path -LiteralPath $archivePath)) {
        $urls = @($item.url) + @($item.mirrors | Where-Object { $_ })
        for ($attempt = 1; $attempt -le 3; $attempt++) {
            try {
                Invoke-WebRequest -UseBasicParsing $urls[($attempt - 1) % $urls.Count] -OutFile "$archivePath.download"
                if ((Get-FileHash -LiteralPath "$archivePath.download" -Algorithm SHA256).Hash.ToLowerInvariant() -ne $item.sha256) { throw "Checksum mismatch for $($item.file)." }
                Move-Item -LiteralPath "$archivePath.download" -Destination $archivePath -Force
                break
            } catch { if ($attempt -eq 3) { throw } }
        }
    }
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
$sharedDir = Join-Path $runtimeDir 'ffmpeg-shared'
Expand-Archive -LiteralPath (Join-Path $runtimeDir 'ffmpeg-shared.zip') -DestinationPath $sharedDir -Force
$sharedArchive = [System.IO.Compression.ZipFile]::OpenRead((Join-Path $runtimeDir 'ffmpeg-shared.zip'))
$sharedRootName = $sharedArchive.Entries[0].FullName.Split('/')[0]
$sharedArchive.Dispose()
$sharedBuild = Get-Item -LiteralPath (Join-Path $sharedDir $sharedRootName)
$avDir = Join-Path $runtimeDir 'av'
New-Item -ItemType Directory -Force $avDir | Out-Null
foreach ($name in @('avutil-61.dll','swresample-7.dll','avcodec-63.dll','avformat-63.dll','swscale-10.dll')) {
    Copy-Item -LiteralPath (Join-Path $sharedBuild.FullName "bin\$name") -Destination $avDir -Force
}
foreach ($item in $manifest.binaries) {
    $actualHash = (Get-FileHash -LiteralPath (Join-Path $runtimeDir $item.path) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $item.sha256) { throw "Binary checksum mismatch: $($item.path)" }
}
Write-Output 'Pinned media runtime ready.'
