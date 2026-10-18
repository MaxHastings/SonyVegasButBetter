$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    python scripts/make_acceptance_media.py
    if ($LASTEXITCODE -ne 0) { throw 'Fixture generation failed' }
    cargo build --release --locked --features gui-test
    if ($LASTEXITCODE -ne 0) { throw 'GUI test build failed' }
    foreach ($stage in @('1','2','qol')) {
        $env:SVBB_GUI_TEST = $stage
        $executable = Join-Path $projectRoot 'target\release\svbb.exe'
        if ($stage -ne '2') { $process = Start-Process $executable -WindowStyle Hidden -PassThru }
        else { $process = Start-Process $executable -ArgumentList ('"' + (Join-Path $projectRoot 'test-output\gui-session.svbb') + '"') -WindowStyle Hidden -PassThru }
        $process.WaitForExit()
        $report = Get-Content (Join-Path $projectRoot "test-output\gui-report-$stage.json") -Raw | ConvertFrom-Json
        if (!$report.passed) { throw "GUI stage $stage failed: $($report.error)" }
        Write-Output "GUI stage $stage passed ($($report.steps) steps)."
    }
} finally {
    Remove-Item Env:\SVBB_GUI_TEST -ErrorAction SilentlyContinue
    Pop-Location
}
