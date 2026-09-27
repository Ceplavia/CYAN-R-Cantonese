@echo off
rem Revert deploy-ime-test.cmd: point the CLSID back at the installed dll.
rem The .ime files + E-series KLID can stay (next installer reuses them).
rem Run elevated.

%windir%\System32\regsvr32.exe /s "C:\Program Files\R-Cantonese\r-cantonese.dll"
%windir%\SysWOW64\regsvr32.exe /s "C:\Program Files\R-Cantonese\r-cantonese-x86.dll"
echo CLSID now points back at the installed dll.
