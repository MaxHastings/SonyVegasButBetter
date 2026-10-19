param([switch]$NoLaunch)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$packageDir = $PSScriptRoot
$runtimeDir = Join-Path $packageDir 'runtime'
$manifest = Get-Content -LiteralPath (Join-Path $packageDir 'runtime-manifest.json') -Raw | ConvertFrom-Json
$logPath = Join-Path $packageDir 'setup.log'

function Write-SetupStatus([string]$message) {
    Write-Host $message
    Add-Content -LiteralPath $logPath -Value "$(Get-Date -Format s) $message"
}
function Test-Runtime([string]$directory) {
    foreach ($item in $manifest.binaries) {
        $path = Join-Path $directory $item.path
        if (!(Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $item.sha256) { return $false }
    }
    return $true
}

try {
    if (![Environment]::Is64BitProcess) { throw 'Use 64-bit Windows PowerShell on Windows 10/11 x64.' }
    if (!(Test-Path -LiteralPath (Join-Path $packageDir 'vulkan-1.dll') -PathType Leaf)) { throw 'The bundled Vulkan loader is missing. Extract the entire release ZIP again.' }
    if (!(Test-Runtime $runtimeDir)) {
        $tar = Join-Path $env:SystemRoot 'System32\tar.exe'
        if (!(Test-Path -LiteralPath $tar)) { throw 'Windows tar.exe is missing. This package requires a current Windows 10/11 installation.' }
        Write-SetupStatus 'First-run media setup: downloading checksum-pinned tools from their upstream GitHub releases (about 250 MB). No administrator access is needed.'
        $cacheDir = Join-Path $packageDir '.setup-cache'
        New-Item -ItemType Directory -Force $cacheDir | Out-Null
        foreach ($item in $manifest.archives) {
            $archive = Join-Path $cacheDir $item.file
            $valid = (Test-Path -LiteralPath $archive -PathType Leaf) -and ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -eq $item.sha256)
            if (!$valid) {
                Write-SetupStatus "Downloading $($item.file)..."
                $download = "$archive.download"
                $downloaded = $false
                for ($attempt = 1; $attempt -le 3; $attempt++) {
                    try {
                        Invoke-WebRequest -UseBasicParsing -Uri $item.url -OutFile $download
                        if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $item.sha256) { throw "Checksum mismatch for $($item.file). Refusing changed media tools." }
                        Move-Item -LiteralPath $download -Destination $archive -Force
                        $downloaded = $true
                        break
                    } catch {
                        if ($attempt -eq 3) { throw }
                        Write-SetupStatus "Download attempt $attempt failed; retrying."
                    }
                }
                if (!$downloaded) { throw "Could not download $($item.file)." }
            }
        }
        $stageDir = Join-Path $cacheDir ([Guid]::NewGuid().ToString())
        New-Item -ItemType Directory $stageDir | Out-Null
        foreach ($item in $manifest.archives) {
            $destination = Join-Path $stageDir ([IO.Path]::GetFileNameWithoutExtension($item.file))
            New-Item -ItemType Directory $destination | Out-Null
            & $tar -xf (Join-Path $cacheDir $item.file) -C $destination
            if ($LASTEXITCODE -ne 0) { throw "Could not extract $($item.file)." }
        }
        $ready = Join-Path $stageDir 'ready'
        New-Item -ItemType Directory -Force (Join-Path $ready 'mpv'),(Join-Path $ready 'av') | Out-Null
        foreach ($item in $manifest.binaries) {
            $leaf = [IO.Path]::GetFileName($item.path)
            $component = if ($item.path.StartsWith('av/')) { 'ffmpeg-shared' } elseif ($item.path.StartsWith('mpv/')) { 'mpv' } else { 'essentials' }
            $matches = @(Get-ChildItem -LiteralPath (Join-Path $stageDir $component) -Recurse -File -Filter $leaf)
            if ($matches.Count -ne 1) { throw "Expected one copy of $leaf in verified upstream archives." }
            Copy-Item -LiteralPath $matches[0].FullName -Destination (Join-Path $ready $item.path)
        }
        if (!(Test-Runtime $ready)) { throw 'Extracted runtime failed verification. Existing files were preserved.' }
        # Keep all upstream licenses/notices with the local download.
        $upstream = Join-Path $ready 'upstream'
        New-Item -ItemType Directory $upstream | Out-Null
        foreach ($name in @('essentials','mpv','ffmpeg-shared')) {
            $source = Join-Path $stageDir $name
            Get-ChildItem -LiteralPath $source -Recurse -File | Where-Object { $_.Name -match '^(license|copying|copyright|readme|notice)' } | ForEach-Object {
                $relative = $_.FullName.Substring($source.Length + 1)
                $target = Join-Path (Join-Path $upstream $name) $relative
                New-Item -ItemType Directory -Force (Split-Path -Parent $target) | Out-Null
                Copy-Item -LiteralPath $_.FullName -Destination $target
            }
        }
        New-Item -ItemType Directory -Force $runtimeDir | Out-Null
        foreach ($child in Get-ChildItem -LiteralPath $ready) { Copy-Item -LiteralPath $child.FullName -Destination $runtimeDir -Recurse -Force }
        if (!(Test-Runtime $runtimeDir)) { throw 'Installed runtime failed verification. Run the launcher again to repair it.' }
        # Only generated staging/download files beneath this package are removed.
        $resolvedCache = [IO.Path]::GetFullPath($cacheDir)
        if ($resolvedCache.StartsWith([IO.Path]::GetFullPath($packageDir).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedCache -Recurse -Force
        }
        Write-SetupStatus 'Media setup complete. Future launches work offline.'
    } else { Write-SetupStatus 'Media runtime verified; no download needed.' }
    if (!$NoLaunch) {
        $env:SVBB_RUNTIME = $runtimeDir
        Start-Process -FilePath (Join-Path $packageDir 'SonyVegasButBetter.exe') -WorkingDirectory $packageDir -WindowStyle Hidden
    }
} catch {
    Write-SetupStatus "Setup failed: $($_.Exception.Message)"
    Write-Host 'Check your connection and extract the entire ZIP into a writable folder such as Documents. Run Start SVBB.cmd again to retry.'
    exit 1
}
