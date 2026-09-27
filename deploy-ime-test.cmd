@echo off
rem Deploy the DEBUG builds for IMM32 testing — no reinstall, no reboot.
rem Run elevated (right-click -> Run as administrator).
rem
rem Effect:
rem   1. .ime files copied into System32 / SysWOW64 (never loaded, overwrite ok)
rem   2. CLSID re-points at target\debug\r_cantonese.dll — every process that
rem      activates the IME loads the debug build (full log output)
rem   3. regsvr32 runs install_imm32_ime -> E-series KLID + hklSubstitute
rem Revert with revert-ime-test.cmd or just reinstall.

setlocal
set ROOT=D:\rust_proj\r-cantonese

rem Hardlink the dictionary next to each debug dll (db resolves via the
rem registered InProcServer32 path); fall back to a real copy.
if not exist "%ROOT%\target\debug\ime.sqlite3" (
        mklink /h "%ROOT%\target\debug\ime.sqlite3" "%ROOT%\rcantonese\ime.sqlite3" 2>nul || copy /y "%ROOT%\rcantonese\ime.sqlite3" "%ROOT%\target\debug\ime.sqlite3"
)
if not exist "%ROOT%\target\i686-pc-windows-msvc\debug\ime.sqlite3" (
        mklink /h "%ROOT%\target\i686-pc-windows-msvc\debug\ime.sqlite3" "%ROOT%\rcantonese\ime.sqlite3" 2>nul || copy /y "%ROOT%\rcantonese\ime.sqlite3" "%ROOT%\target\i686-pc-windows-msvc\debug\ime.sqlite3"
)

copy /y "%ROOT%\target\debug\r_cantonese_ime.dll" "%windir%\System32\r-cantonese.ime"
copy /y "%ROOT%\target\i686-pc-windows-msvc\debug\r_cantonese_ime.dll" "%windir%\SysWOW64\r-cantonese.ime"

"%windir%\System32\regsvr32.exe" /s "%ROOT%\target\debug\r_cantonese.dll"
"%windir%\SysWOW64\regsvr32.exe" /s "%ROOT%\target\i686-pc-windows-msvc\debug\r_cantonese.dll"

echo.
echo === E-series KLID registered for r-cantonese.ime: ===
for /f "tokens=1" %%K in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts" /s /f "r-cantonese.ime" /d ^| findstr /i "Keyboard.Layouts.E"') do echo %%K
echo.
echo Done. Restart WoW and test. Log: %%LOCALAPPDATA%%\RCantonese\Logs\RCantonese.log
endlocal
