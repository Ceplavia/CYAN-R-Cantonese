; R-Cantonese 輸入法 — Inno Setup installer script
; Build: ISCC.exe installer.iss  (expects release binaries in target\release)

#define AppName "R-Cantonese"
#define AppVersion "0.9.0"
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
CloseApplications=yes
CloseApplicationsFilter=r-cantonese-tray.exe,config-center.exe
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
Source: "rcantonese\ime.sqlite3"; DestDir: "{app}"; Flags: ignoreversion
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
Filename: "{app}\r-cantonese-tray.exe"; Flags: runasoriginaluser nowait
; Config center opens at the end so the user can tweak or just close it.
Filename: "{app}\config-center.exe"; Flags: runasoriginaluser nowait skipifsilent

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

// On uninstall: kill helpers first, let [UninstallRun] unregister, then
// after file removal offer to delete per-user data.
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
