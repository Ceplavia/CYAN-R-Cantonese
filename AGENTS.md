
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
