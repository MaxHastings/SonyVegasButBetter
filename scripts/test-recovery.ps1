$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    cargo build --locked --features gui-test
    if ($LASTEXITCODE -ne 0) { throw 'Recovery test build failed' }
    Copy-Item -LiteralPath 'test-output\gui-session.svbb' -Destination 'test-output\gui-crash-source.svbb' -Force
    $env:SVBB_GUI_TEST = 'crash-write'
    $executable = Join-Path $projectRoot 'target\debug\svbb.exe'
    $process = Start-Process $executable -ArgumentList ('"' + (Join-Path $projectRoot 'test-output\gui-crash-source.svbb') + '"') -PassThru
    $process.WaitForExit()
    if ($process.ExitCode -ne 86) { throw "Expected intentional termination 86; got $($process.ExitCode)" }
    $written = Get-Content 'test-output\gui-report-crash-write.json' -Raw | ConvertFrom-Json
    if (!$written.passed) { throw 'Recovery write phase failed' }
    $env:SVBB_GUI_TEST = 'crash-read'
    $process = Start-Process $executable -PassThru
    $process.WaitForExit()
    $read = Get-Content 'test-output\gui-report-crash-read.json' -Raw | ConvertFrom-Json
    if (!$read.passed) { throw "Recovery read phase failed: $($read.error)" }
    Write-Output 'Intentional termination and native recovery round trip passed; manual save remained unchanged.'
} finally {
    Remove-Item Env:\SVBB_GUI_TEST -ErrorAction SilentlyContinue
    Pop-Location
}
