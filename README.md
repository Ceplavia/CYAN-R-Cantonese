# R-Cantonese

用 Rust 寫嘅 Windows 粵拼輸入法。

<p align="center">★ 特別鳴謝 ★</p>

[rime-cantonese](https://github.com/rime/rime-cantonese) | [yuetyam/jyutping-windows](https://github.com/yuetyam/jyutping-windows)

<p align="center">★ 特別鳴謝 ★</p>

## 有咩好用

- **粵拼打字** — 打 `nei hou` 出「你好」，支援詞語、多字輸入、自動分詞
- **智能學習** — 你揀過嘅候選會記住，打多幾次就自動排前（`memory.sqlite3`）
- **反查功能** — 識粵拼唔夠？用其它輸入法反查都得：
  - `` ` ``開頭 — 普通話拼音反查
  - `v` 開頭 — 倉頡反查
  - `x` 開頭 — 筆畫反查
  - `q` 開頭 — 結構（速成）反查
- **`/` 符號查詢** — `/` 開頭出符號表：`/數字` 出全形數字變體、`/字母` 反查單字粵拼 + emoji
- **候選窗自訂** — 字號、編號字號、註釋字號、每頁數目、顏色全部可以喺配置中心改
- **字符集切換** — 繁體（香港）/ 繁體（台灣）/ 簡體，候選字跟住變
- **標點模式** — 中文標點（全形 `，。？`）定英文標點（ASCII `,.?`）任你揀
- **半寬/全寬** — 數字字母寬度切換
- **配置中心** — 圖形界面改晒所有設定，套用即時生效

## 快捷鍵

| 掣 | 做咩 |
|----|------|
| `Ctrl` + `` ` `` | （預設）開選項 menu（字體/標點/字符集）|
| `Ctrl` + `.` | 中文 / 英文標點切換 |
| `Shift` + `Space` | 半寬 / 全寬切換 |
| `Ctrl` + `Shift` + `Delete` | 移除當前選中詞嘅權重 |

## 安裝

行 `r-cantonese-setup-x.x.x.exe`：

1. 揀安裝路徑（預設 `C:\Program Files\R-Cantonese`）
2. 自動註冊輸入法去「中文（香港）」
3. 裝完自動開配置中心 — 想改嘢就改咗佢撳「套用」，冇嘢改就直接關（以後可以喺托盤區 right click 開返配置中心）
4. `Win + Space` 切去 R-Cantonese 即刻用得

> 提示：tray icon 喺通知區嘅「^」收起區入面 — 拖出嚟就可以常駐顯示。

### 結構

| 目錄 | 做咩 |
|------|------|
| `rcantonese/` | IME 主體 |
| `tray/` | 獨立 tray 進程 |
| `config-center/` | `config-center.exe` — 配置界面 |
| `preparing/` | 字典 DB 生成器 |

## License

CC0 1.0 Public Domain — 隨便用。
