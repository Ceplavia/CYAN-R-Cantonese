
## 架構筆記（2026-09-20 更新）

### Log
- 路徑 `%LOCALAPPDATA%\RCantonese\Logs\RCantonese.log`（同 settings.toml 一齊）
- release build 淨係寫 `log_error`；`log` 係 debug-only

### Shift / preserved key
- `OnPreservedKey` 時仲 compose 緊 → 先 queue async edit session commit raw input，先至 flip compartment（唔做嘅話 composition 吊喺度，之後撳 space 會出真空格）
- langbar compartment sink `OnChange` **唔可以掂 TSF**（SetValue broadcast 入面 call `OnUpdate`/`GetValue` 會喺某啲 app 死鎖）— 只係 `PostMessage` 去 per-thread message-only window（`RCantoneseRefreshWnd`），`langbar::deferred_refresh` 先至做 `notify_update` + `tray::update_mode`

### Tray icon（Weasel 式）
- langbar item 用 **`GUID_LBI_INPUTMODE`**（保留 GUID）+ `TF_LBI_STYLE_SHOWNINTRAY` = 輸入法圖標隔籬嘅「中/A」— 主圖標，左撳 toggle、右撳 InitMenu。自訂 GUID 註冊到但 Win8+ 唔會 render
- `r-cantonese-tray.exe` 仲係 message router + balloon host，但 icon 默認收埋（`display_tray_icon = true` 先常駐）
- balloon 要 icon 先彈到 — `balloon_until` 臨時頂起 icon 10s，prune timer 到期自動收
- config-center 有「托盤圖標」ToggleSwitch

### InstallLayoutOrTip 嘅 context 陷阱
- **elevated regsvr32 入面 call 會落去 `.DEFAULT` hive** — tip 唔會入到用戶語言清單，仲會喺 .DEFAULT 加埋「關聯預設鍵盤」（en-US/en-HK/zh 系列 US layout → flyout 幻影 US entry）
- 所以 `register_profiles` 入面 elevated 會 skip；**tray.exe 啟動時 `ensure_input_tip()`** 喺 user context 補上（idempotent，login 自啟）
- uninstall 方向 elevated 係 work 嘅 — `install_layout_or_tip(true)` 保留喺 `DllUnregisterServer`
- 幻影殘留位：`User Profile\Languages`（multi_sz）+ `User Profile\<lang>` key + `CTF\SortOrder\AssemblyItem` + `.DEFAULT` 同位 — 全部要 surgical 清

### 32-bit 程序
- `r-cantonese.dll` 係 x64 — 32-bit app 物理上 load 唔到（「注入失敗切唔到」嘅元兇）
- i686 build：`cargo build --release -p r-cantonese --target i686-pc-windows-msvc` → `target\i686-pc-windows-msvc\release\r_cantonese.dll`
- 註冊：`C:\Windows\SysWOW64\regsvr32.exe /s r-cantonese-x86.dll` → CLSID 入 WOW6432Node
- `winsqlite3` 用 `kind = "raw-dylib"` — 唔使 SDK import lib
- installer 包 `r-cantonese-x86.dll`，uninstall 用 SysWOW64 regsvr32 /u

### ✅ x86 crash 已修（2026-09-21）— winsqlite3 stdcall
**根因**：Microsoft 嘅 `winsqlite3.dll` 喺 x86 係用 `/Gz` 編 — **全部 export 係 stdcall**（`retl $N` callee-clean）。我哋 `db.rs` declare 做 `extern "C"`（cdecl）→ 每次 call：callee 清 args + caller 又清一次 → ESP 向上漂移 → `ret` 擸到垃圾地址亂跳（解釋晒跳字串/heap/null EIP）。x64 得一款 convention 所以冇事。

**修法**（`db.rs`）：
```rust
#[cfg_attr(target_arch = "x86", link(name = "winsqlite3", kind = "raw-dylib", import_name_type = "undecorated"))]
#[cfg_attr(not(target_arch = "x86"), link(name = "winsqlite3", kind = "raw-dylib"))]
unsafe extern "system" { ... }   // system = stdcall on x86, = C on x64
```
- `extern "system"` 唔係 `extern "C"` — 因為 winsqlite3 係 stdcall
- `import_name_type = "undecorated"` — winsqlite3 有 .def，export 名係 plain `sqlite3_*` 唔係 `_name@N`
- `import_name_type` attribute 淨係 x86 接受 → 要 `cfg_attr` 分開兩個 arch
- callback 都要 `extern "system"`（sqlite 內部都係 stdcall call 我哋）

**repro32 測試工具**（`repro32/` workspace member）：
- 32-bit TSF host — CoCreateInstance + ActivateEx + CreateDocumentMgr/Push 行成個注入路徑，唔使 UAC 唔使真 app
- VEH 內置 minidump + StackWalk64 + module scan
- `REPRO32_NOTHING` / `REPRO32_SQLITE_ONLY` env 做隔離測試
- HKCU per-user CLSID shadow（32-bit reg.exe 寫嘅 `HKCU\Software\Classes\CLSID\{...}\InProcServer32`）指去 dev dll → 迭代唔使郁 Program Files

**Debug gates**（debug-only，release 唔會生效）：
- `RCANTONESE_SKIP_ENGINE`/`_MEMORY`/`_TRAY`/`_COMPARTMENTS`/`_PRESERVED`/`_LANGBAR` env — 二分用
- `AdviseKeyEventSink` 失敗喺 debug build 唔會 early-return（synthetic host 冇真 input queue）

**⚠️ 機器狀態**：HKCU shadow 仲喺度，兩個 arch 都指去 dev debug dll（log 全開）：
- x64: `HKCU\Software\Classes\CLSID\{...}` → `target\debug\r_cantonese.dll`
- x86: WOW6432Node view 同位 → `target\i686-pc-windows-msvc\debug\r_cantonese.dll`
- `target\debug\ime.sqlite3` + `target\i686-...\debug\ime.sqlite3` 已 copy（engine 要 db 喺 dll 隔籬）
- 裝咗正件 + reboot 之後先刪 shadow（否則 app 繼續行 dev dll）

### ✅ 後續修咗（2026-09-21）
- **Release 冇候選**：`RCANTONESE_SKIP_ENGINE` gate 寫反 — `cfg!(debug_assertions)` false → else 永遠行 → engine 永遠 None。改返 `is_ok()` 先 skip。
- **右撳 menu 唔跟 click-away**：經典 Q135788 — popup owner 要 `SetForegroundWindow` 先至有 input focus；仲改埋 owner 做 worker thread 自己起嘅 hidden window（之前 `GetForegroundWindow()` 係外國 thread window，menu 可能閃爍即逝）。**Shell 右撳確實 call `OnClick(TF_LBI_CLK_RIGHT)`，唔係 InitMenu**（log 證實）。
- **`.DEFAULT` phantom en-US/en-HK**：`User Profile\Languages` + `Preload` 俾嘢寫 — 冇提權清唔到；疑係 elevated `RegisterProfile` 嘅 `bEnableByDefault`/associated-keyboard 副作用（ILOT 已 gate）。待辦：installer `[Code]` elevated cleanup step。
- **Install 後舊 dll 仲行緊**：`restartreplace` 排 `PendingFileRenameOperations` — 要 reboot 先換（或者 per-user shadow 繞過）。

## Workflow
- **唔好主動 `git push`** — commit 照做，push 等用戶明確指示（減少 GitHub history 噪音）

## 已知問題（2026-09-19 記錄）

### Tray icon 唔穩定
- 重開 app 之後「中」又消失（殭屍 worker 探針有做，但仲有其它 path 會死）
- 喺其它注入咗嘅 process（例如 QQ）切輸入法切唔出嚟；再開返 Notepad tray 又冇
- 可能方向：claim/visible poll 嘅時序、focus event 次序、process 之間 HKL 快取、QQ 類 app 嘅 IME switch 行為

### 右鍵菜單
- 我哋自己嘅 tray icon 先有右鍵 menu — 係正常（所有 IME 嘅 langbar 圖標本身冇右鍵）
- TF langbar menu（Windows 自動加「R-Cantonese v0.8.0」尾項嗰個）入面 MORE_SETTINGS 有冇出要驗證 — menu_items() 係共用嘅

### 部署
- DLL: target\debug\r-cantonese.dll（由 r_cantonese.dll copy）— 舊版備份 r-cantonese-oldNN.dll
- Config center: target\debug\config-center.exe（windows-reactor + WinUI3 self-contained）
- 改完要重開被注入 app 先 load 新 DLL
- 測試機用戶系統語言係英文 — menu 語言跟 OS/ui_language 設定
