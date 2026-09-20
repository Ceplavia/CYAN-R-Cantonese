; R-Cantonese 輸入法 — Inno Setup installer script
; Build: ISCC.exe installer.iss  (expects release binaries in target\release)

#define AppName "R-Cantonese"
#define AppVersion "0.9.2"
#define AppPublisher "R-Cantonese Project"
; Fixed AppId → in-place upgrades detect the existing install.
#define AppId "{{8F2C4E6A-3B71-4D9A-9E5F-7C1A2B3D4E5F}"

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\R-Cantonese
DefaultGroupName={#AppName}
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/ultra64
SolidCompression=yes
OutputDir=target\installer
OutputBaseFilename=r-cantonese-setup-{#AppVersion}
WizardStyle=modern
SetupIconFile=rcantonese\resources\jyutping.ico
; The DLL may still be loaded in host processes → restartreplace below.
; CloseApplications stays OFF — the Restart Manager session in wpPreparing
; hangs/crashes setup on some machines; our taskkill in ssInstall stops the
; helpers instead, and injected hosts are handled by restartreplace.
CloseApplications=no
RestartIfNeededByRun=no
UninstallDisplayName={#AppName} 輸入法
VersionInfoVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; The IME DLL may be loaded in injected processes — restartreplace queues
; the new image for the next reboot instead of failing the copy.
Source: "target\release\r_cantonese.dll"; DestDir: "{app}"; DestName: "r-cantonese.dll"; Flags: restartreplace ignoreversion
; 32-bit twin — 32-bit processes can't load a 64-bit COM dll (this is why
; injection "failed" in some apps: they were x86). Registered via SysWOW64
; regsvr32 → WOW6432Node.
Source: "target\i686-pc-windows-msvc\release\r_cantonese.dll"; DestDir: "{app}"; DestName: "r-cantonese-x86.dll"; Flags: restartreplace ignoreversion
Source: "target\release\r-cantonese-tray.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "target\release\config-center.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "rcantonese\ime.sqlite3"; DestDir: "{app}"; Flags: ignoreversion restartreplace
; WinUI3 self-contained runtime for config-center (en-US + zh locales only).
Source: "target\release\Microsoft.*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\CoreMessagingXP.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\DWriteCore.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\DwmSceneI.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\MRM.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\marshal.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\wuceffectsi.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\dcompi.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\dwmcorei.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\WinUIEdit.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\*.pri"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\Microsoft.UI.Xaml\*"; DestDir: "{app}\Microsoft.UI.Xaml"; Flags: recursesubdirs ignoreversion skipifsourcedoesntexist
Source: "target\release\en-US\*"; DestDir: "{app}\en-US"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\zh-HK\*"; DestDir: "{app}\zh-HK"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\zh-TW\*"; DestDir: "{app}\zh-TW"; Flags: ignoreversion skipifsourcedoesntexist
Source: "target\release\zh-CN\*"; DestDir: "{app}\zh-CN"; Flags: ignoreversion skipifsourcedoesntexist

[Run]
; Register the IME (CLSID + TSF profile + categories + InstallLayoutOrTip
; enable + tray autostart Run key — all inside DllRegisterServer).
Filename: "regsvr32.exe"; Parameters: "/s ""{app}\r-cantonese.dll"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering R-Cantonese..."
; 32-bit view of regsvr32 → writes WOW6432Node CLSID + TIP for x86 apps.
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/s ""{app}\r-cantonese-x86.dll"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering R-Cantonese (32-bit)..."
; Tray host starts now (un-elevated, as the installing user) and at login.
; postinstall → launched from the Finish page's checkbox, so a slow/hung
; spawn can never freeze the wizard mid-install.
Filename: "{app}\r-cantonese-tray.exe"; Flags: runasoriginaluser nowait postinstall
; Config center opens at the end so the user can tweak or just close it.
Filename: "{app}\config-center.exe"; Flags: runasoriginaluser nowait postinstall skipifsilent

[UninstallRun]
; Before files are deleted: unregister the IME (drops the input tip, the
; TSF profile, the CLSID entry and the tray autostart value).
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/s /u ""{app}\r-cantonese-x86.dll"""; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterIME32"
Filename: "regsvr32.exe"; Parameters: "/s /u ""{app}\r-cantonese.dll"""; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterIME"

[Code]
procedure KillHelper(const ExeName: String);
var
  ResultCode: Integer;
begin
  Exec('taskkill.exe', '/f /im ' + ExeName, '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

// Stop the helpers before install/upgrade replaces their files.
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    KillHelper('r-cantonese-tray.exe');
    KillHelper('config-center.exe');
  end;
end;

// Delete-on-reboot fallback for files still locked by injected processes.
function MoveFileExW(Existing: String; NewName: Integer; Flags: DWORD): BOOL;
  external 'MoveFileExW@kernel32.dll stdcall';
const MOVEFILE_DELAY_UNTIL_REBOOT = $4;

// Files the IME leaves locked at uninstall time (the dll stays mapped in
// injected processes; ime.sqlite3 stays open). Renaming a loaded file is
// allowed, so move leftovers out of {app} and queue them for deletion on
// reboot — the install dir then comes out clean.
procedure SweepLockedLeftovers();
var
  FindRec: TFindRec;
  AppDir, Path, Staging: String;
begin
  AppDir := ExpandConstant('{app}');
  Staging := ExpandConstant('{tmp}\r-cantonese-leftover');
  ForceDirectories(Staging);
  if FindFirst(AppDir + '\r-cantonese*.dll', FindRec) then
  begin
    try
      repeat
        Path := AppDir + '\' + FindRec.Name;
        if not DeleteFile(Path) then
        begin
          if not RenameFile(Path, Staging + '\' + FindRec.Name) then
            MoveFileExW(Path, 0, MOVEFILE_DELAY_UNTIL_REBOOT);
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
  if FindFirst(AppDir + '\ime.sqlite3', FindRec) then
  begin
    try
      repeat
        Path := AppDir + '\' + FindRec.Name;
        if not DeleteFile(Path) then
        begin
          if not RenameFile(Path, Staging + '\' + FindRec.Name) then
            MoveFileExW(Path, 0, MOVEFILE_DELAY_UNTIL_REBOOT);
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
  RemoveDir(AppDir);
end;

// On uninstall: kill helpers first, let [UninstallRun] unregister, then
// after file removal sweep locked leftovers and offer to delete per-user data.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    KillHelper('r-cantonese-tray.exe');
    KillHelper('config-center.exe');
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    SweepLockedLeftovers();
    // Log dir — always safe to remove.
    DataDir := ExpandConstant('%TEMP%\RCantonese');
    if DirExists(DataDir) then
      DelTree(DataDir, True, True, True);
    // User data (settings.toml, memory.sqlite3 learning DB) — ask first.
    DataDir := ExpandConstant('{localappdata}\RCantonese');
    if DirExists(DataDir) then
      if MsgBox('Delete user data (settings and learned words)?' + #13#10 +
                '刪除用戶數據（設定同埋學習記錄）？',
                mbConfirmation, MB_YESNO) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
