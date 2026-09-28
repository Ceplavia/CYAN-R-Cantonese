@echo off
rem Revert dev deploys: point the CLSID back at the installed dll.
rem Run elevated.

%windir%\System32\regsvr32.exe /s "%windir%\System32\r-cantonese.dll"
%windir%\SysWOW64\regsvr32.exe /s "%windir%\SysWOW64\r-cantonese-x86.dll"
echo CLSID now points back at the installed dll.
