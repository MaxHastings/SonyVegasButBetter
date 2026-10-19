$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$projectRoot = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $projectRoot 'runtime'
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'runtime-manifest.json') -Raw | ConvertFrom-Json
$sources = Join-Path $runtime 'vulkan-source'
New-Item -ItemType Directory -Force $runtime,$sources | Out-Null
foreach ($item in $manifest.vulkan_loader.source_archives) {
    $archive = Join-Path $runtime $item.file
    if (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest -UseBasicParsing $item.url -OutFile $archive }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $item.sha256) { throw "Changed Vulkan source archive: $($item.file)" }
    if (!(Test-Path -LiteralPath (Join-Path $sources $item.root))) { Expand-Archive -LiteralPath $archive -DestinationPath $sources }
}
$headers = Join-Path $sources $manifest.vulkan_loader.source_archives[1].root
$loader = Join-Path $sources $manifest.vulkan_loader.source_archives[0].root
$headerBuild = Join-Path $runtime 'vulkan-headers-build'
$headerInstall = Join-Path $runtime 'vulkan-headers-install'
$loaderBuild = Join-Path $runtime 'vulkan-build'
& cmake -S $headers -B $headerBuild "-DCMAKE_INSTALL_PREFIX=$headerInstall"
if ($LASTEXITCODE -ne 0) { throw 'Vulkan headers configure failed' }
& cmake --install $headerBuild --config Release
if ($LASTEXITCODE -ne 0) { throw 'Vulkan headers install failed' }
& cmake -S $loader -B $loaderBuild -A x64 -DBUILD_TESTS=OFF -DUPDATE_DEPS=OFF "-DCMAKE_PREFIX_PATH=$headerInstall" -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
if ($LASTEXITCODE -ne 0) { throw 'Vulkan loader configure failed' }
& cmake --build $loaderBuild --config Release --parallel 4
if ($LASTEXITCODE -ne 0) { throw 'Vulkan loader build failed' }
Copy-Item -LiteralPath (Join-Path $loaderBuild 'loader\Release\vulkan-1.dll') -Destination (Join-Path $runtime 'vulkan-1.dll') -Force
Write-Output 'App-local Vulkan loader built from verified sources; no system installation.'
