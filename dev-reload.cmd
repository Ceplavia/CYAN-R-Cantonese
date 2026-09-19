@echo off
REM dev-reload.cmd — rebuild the IME DLL + tray exe, redeploy, and force
REM injected processes to drop the old image. Run from an elevated shell.
REM
REM Usage:
REM   dev-reload.cmd          build + deploy + restart explorer + tray
REM   dev-reload.cmd /reg     also re-register the COM server + TSF profile
REM                           (needed only after CLSID/GUID/langid changes)

setlocal
set "ROOT=%~dp0"
set "DLLDIR=%ROOT%target\debug"
set "DLL=%DLLDIR%\r-cantonese.dll"

echo [1/4] Building...
cargo build -p r-cantonese -p r-cantonese-tray || exit /b 1

echo [2/4] Deploying DLL + tray exe...
if exist "%DLL%" move "%DLL%" "%DLLDIR%\r-cantonese-old-%RANDOM%.dll" >nul 2>&1
copy /y "%DLLDIR%\r_cantonese.dll" "%DLL%" >nul || exit /b 1

if /i "%~1"=="/reg" (
        echo [3/4] Re-registering COM + TSF profile...
        regsvr32 /s /u "%DLL%" || echo   unregister failed (continuing)
        regsvr32 /s "%DLL%" || exit /b 1
) else (
        echo [3/4] Skipping re-registration (pass /reg to force).
)

echo [4/4] Restarting tray exe + explorer (drops old images)...
taskkill /f /im r-cantonese-tray.exe >nul 2>&1
taskkill /f /im explorer.exe >nul 2>&1
start "" explorer.exe

echo.
echo Done. Restart your test app (e.g. Notepad) so it loads the new DLL.
endlocal
