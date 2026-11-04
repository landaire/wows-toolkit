@echo off
rem Runs the Buck2 release that toolchains\buck2 pins, through the DotSlash
rem that bootstrap.ps1 installs into .tools. Add this directory to PATH to
rem run it as buck2.
setlocal
for %%I in ("%~dp0..") do set "ROOT=%%~fI"

if not exist "%ROOT%\.tools\bin\dotslash.exe" (
    rem Bootstrap messages go to stderr so they do not mix with Buck2 output.
    powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\bootstrap.ps1" -Buck2Only 1>&2 || exit /b 1
)

set "DOTSLASH_CACHE=%ROOT%\.tools\dotslash"
"%ROOT%\.tools\bin\dotslash.exe" "%ROOT%\toolchains\buck2" %*
exit /b %ERRORLEVEL%
