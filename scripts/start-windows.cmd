@echo off
setlocal
rem Let Windows PowerShell use its own modules if launched from PowerShell 7.
set "PSModulePath="
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup-runtime.ps1" %*
if errorlevel 1 (
  echo.
  echo Setup could not finish. Check the message above and setup.log, then try again.
  pause
  exit /b 1
)
