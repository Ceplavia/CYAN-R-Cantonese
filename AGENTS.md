
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

### ⚠️ x86 DLL 注入會 crash host — 調查中（2026-09-20 深夜）
**病徵**：任何 32-bit process 激活 IME（TSF inject `r-cantonese-x86.dll`）→ 成個 process crash（0xC0000005 → 0xC000041d）。Inno setup 係 32-bit → 裝 0.9.x 嗰陣 wizard 起 UI → 注入 → 閃退（「Preparing to Install」停住 ~18s 係 WER 寫 dump）。**呢個先係「部分程序注入失敗」嘅真正根因** — 唔係 load 唔到，係 load 咗但 crash 埋個 host。

**已排除**：
- installer/RM/`runasoriginaluser`/`postinstall` — 唔關事（裸 Inno mini-setup 都死，`/VERYSILENT` 冇 UI 反而唔死 → 證實係注入 path）
- Defender exclusion — 冇效
- `extern "system"` calling convention — 全部 callback 啱
- `winsqlite3` import 名 — undecorated 正確綁定，`sqlite3_open_v2` 返回 0

**收窄到嘅位置**：debug log 最後停喺 `db::open result=0`（`db.rs`）— 即係 `engine::prepare`/`memory.prepare`/`tray::activate` 段。但 inline 路徑冇嘢可以爆 → 疑係 stack corruption 或別 thread。

**Dump 分析**（LocalDumps + 手寫 minidump parser）：
- release crash：IAT `memcpy` slot（VCRUNTIME140，RVA 0xB30C4）俾人寫咗 heap ptr 0x07167018 → thunk jmp 爆
- debug crash：EIP=「engine::prepare after open_default」**字串 literal 地址**（.rdata）— 跳咗去 data 執行，疑似 `ret`/indirect call 擸咗 arg 做 target → **stack imbalance / wild jump**
- 兩個 dump 一致指向「跳去 data pointer」— 疑係：間接 call target 錯位、vtable slot 錯、或 callee ret N 令 `ret` 擸到 arg
- stack 上有 `engine::prepare`（0x10090e10 範圍）+ landing pad frames — crash 喺 activation thread inline

**調查工具**（都喺機上裝好）：
- WER LocalDumps：`HKLM\...\LocalDumps\<exe-name>.tmp` → `%TEMP%\dumps\`（DumpType=2）
- dump parser：`/tmp/dump*.ps1`（PowerShell + C# 手寫 minidump parser，攞 EIP/module/stack）
- `llvm-objdump.exe`/`llvm-readobj.exe`/`llvm-nm.exe`：`rustup component add llvm-tools` 已裝，用嚟反匯編 + map offset
- Debug x86 dll 已 deploy 喺 `C:\Program Files\R-Cantonese\r-cantonese-x86.dll`（release 改名 `r-cantonese-x86-rel.dll`）；log 有晒 breadcrumb
- 重現：`C:\Users\shagg\AppData\Local\Temp\mini-setup.exe`（裸 Inno installer，零 code — 起 UI 即注入我哋個 dll）

**下一步**：
- processor.rs 有 `RCANTONESE_SKIP_ENGINE` debug gate（`processor.engine` skip）— 但 UAC 洗走 env var，要諗辦法傳入 elevated process（或者直接 hardcode skip 試）
- 逐段二分：`setup_language_bar`/`engine`/`memory`/`tray::activate` 逐個 comment 試
- 或者檢查 windows crate 0.62.2 嘅 i686 `#[implement]` vtable — shell call 我哋 COM method 時如果 vtable slot 錯位會直接跳入 data

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
