// r-cantonese-tray.exe — standalone tray-icon host, Weasel-style.
//
// The IME DLL never owns the icon itself: whichever process activates the
// IME just posts WM_TRAY_SHOW (its pid) to this worker; WM_TRAY_HIDE removes
// it. The icon is visible while the pid set is non-empty, so a process that
// dies without hiding is pruned by the liveness timer — zombies are
// impossible by construction.
//
// Clicks are forwarded to the IME host windows (RCantoneseTrayHostWnd, one
// per injected process): left = zh/abc toggle, right = settings menu. When
// no host window exists (all IME instances deactivated) a minimal local
// menu still offers the config-center entry.

#![windows_subsystem = "windows"]

use std::collections::HashSet;
use std::sync::Mutex;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};

use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY_ICON: u32 = WM_APP + 40; // tray callback
const WM_TRAY_SETMODE: u32 = WM_APP + 41; // wParam: 1 = zh, 0 = abc
const WM_TRAY_QUIT: u32 = WM_APP + 42;
const WM_TRAY_TOGGLE: u32 = WM_APP + 43; // hosts: toggle zh/abc
const WM_TRAY_MENU: u32 = WM_APP + 44; // hosts: show settings menu
const WM_TRAY_NOTIFY: u32 = WM_APP + 46; // balloon (wParam: 1=applied, 2=reloaded, 3=memory cleared)
const WM_TRAY_PING: u32 = WM_APP + 48; // liveness probe — returns TRAY_MAGIC
const WM_TRAY_UPDATE: u32 = WM_APP + 49; // re-add/refresh the icon
const WM_TRAY_SHOW: u32 = WM_APP + 50; // wParam = pid — mark IME active
const WM_TRAY_HIDE: u32 = WM_APP + 51; // wParam = pid — mark IME inactive
const TRAY_MAGIC: isize = 0x5243; // 'RC'
const PRUNE_TIMER_ID: usize = 1;

const TRAY_WND_CLASS: PCWSTR = w!("RCantoneseTrayIconWnd");
const TRAY_ICON_ID: u32 = 0x4341;
const IDM_CONFIG_CENTER: u32 = 1;

struct State {
        pids: HashSet<u32>,
        /// Most recent pid to post SHOW — the right-click menu is routed to
        /// its host window (it's the process the user is interacting with).
        last_shown: u32,
        icon_added: bool,
        mode_is_zh: bool,
        /// Balloon texts received while the icon isn't in the notification
        /// area — NIF_INFO needs the icon to exist, so they're queued and
        /// flushed when it gets added.
        pending_balloons: Vec<String>,
        /// `display_tray_icon` from settings.toml — Weasel-style default off:
        /// the langbar item beside the input indicator is the primary icon,
        /// this one only surfaces for balloons unless the user opts in.
        display_tray_icon: bool,
        /// While set (and in the future) the icon stays up to display a
        /// balloon even when `display_tray_icon` is off.
        balloon_until: Option<std::time::Instant>,
}

static STATE: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(|| {
        Mutex::new(State {
                pids: HashSet::new(),
                last_shown: 0,
                icon_added: false,
                mode_is_zh: true,
                pending_balloons: Vec::new(),
                display_tray_icon: read_display_tray_icon(),
                balloon_until: None,
        })
});

/// Read `display_tray_icon` straight from settings.toml — the tray exe has
/// no TOML dep, so it's a plain key scan of the flat file.
fn read_display_tray_icon() -> bool {
        let Some(base) = std::env::var_os("LOCALAPPDATA") else {
                return false;
        };
        let path = std::path::PathBuf::from(base)
                .join("RCantonese")
                .join("settings.toml");
        let Ok(content) = std::fs::read_to_string(&path) else {
                return false;
        };
        for line in content.lines() {
                let line = line.split('#').next().unwrap_or("").trim();
                if let Some((key, value)) = line.split_once('=') {
                        if key.trim() == "display_tray_icon" {
                                return matches!(value.trim(), "true" | "1" | "yes");
                        }
                }
        }
        false
}

/// Image name of a pid's process — tells a real tray.exe window apart from
/// a stale in-process worker left by an older injected DLL build.
fn process_name(pid: u32) -> String {
        unsafe {
                let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                        return String::new();
                };
                let mut buf = [0u16; 260];
                let mut len = buf.len() as u32;
                let name = if QueryFullProcessImageNameW(
                        h,
                        PROCESS_NAME_FORMAT(0),
                        PWSTR(buf.as_mut_ptr()),
                        &mut len,
                )
                .is_ok()
                {
                        String::from_utf16_lossy(&buf[..len as usize])
                } else {
                        String::new()
                };
                let _ = CloseHandle(h);
                name.rsplit('\\').next().unwrap_or("?").to_string()
        }
}

/// Every window with our class — normally 0-1 entries; stale in-process
/// workers from older injected builds share the class name.
fn class_windows() -> Vec<HWND> {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let hwnds = &mut *(lparam.0 as *mut Vec<isize>);
                let mut class = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                if String::from_utf16_lossy(&class[..n]) == "RCantoneseTrayIconWnd" {
                        hwnds.push(hwnd.0 as isize);
                }
                BOOL(1)
        }
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
                let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<isize> as isize));
        }
        hwnds.into_iter().map(|h| HWND(h as *mut std::ffi::c_void)).collect()
}

fn main() {
        unsafe {
                // Single instance among tray.exe-owned windows; stale
                // in-process workers get told to quit instead of blocking us.
                for w in class_windows() {
                        let mut pid = 0u32;
                        GetWindowThreadProcessId(w, Some(&mut pid));
                        if process_name(pid).eq_ignore_ascii_case("r-cantonese-tray.exe") {
                                return; // another tray.exe already owns it
                        }
                        let _ = PostMessageW(Some(w), WM_TRAY_QUIT, WPARAM(0), LPARAM(0));
                }
                let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                let mut wc = WNDCLASSEXW::default();
                wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                wc.lpfnWndProc = Some(wnd_proc);
                wc.hInstance = instance;
                wc.lpszClassName = TRAY_WND_CLASS;
                let _ = RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        TRAY_WND_CLASS,
                        PCWSTR::null(),
                        WS_POPUP,
                        0, 0, 0, 0,
                        None,
                        None,
                        Some(instance),
                        None,
                )
                .unwrap_or_default();
                if hwnd.is_invalid() {
                        log_error(&format!("CreateWindowExW failed err={:?}", GetLastError()));
                        return;
                }
                // Another tray.exe may have raced us — only a window owned by
                // a real tray.exe (not a stale in-process worker) blocks us.
                for w in class_windows() {
                        if w == hwnd {
                                continue;
                        }
                        let mut pid = 0u32;
                        GetWindowThreadProcessId(w, Some(&mut pid));
                        if process_name(pid).eq_ignore_ascii_case("r-cantonese-tray.exe") {
                                log("another tray.exe owns the window — exiting");
                                return;
                        }
                        let _ = PostMessageW(Some(w), WM_TRAY_QUIT, WPARAM(0), LPARAM(0));
                }
                log(&format!("tray.exe: started hwnd={:?}", hwnd));
                queue_first_run_hint();
                let _ = SetTimer(Some(hwnd), PRUNE_TIMER_ID, 4000, None);
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                }
        }
}

fn log(s: &str) {
        let _ = s;
        #[cfg(debug_assertions)]
        write_log(s);
}

fn log_error(s: &str) {
        write_log(&format!("ERROR: {s}"));
}

fn write_log(s: &str) {
        // Same file the injected DLL writes — %LOCALAPPDATA%\RCantonese\Logs.
        let base = std::env::var_os("LOCALAPPDATA")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
        let dir = base.join("RCantonese").join("Logs");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("RCantonese.log")) {
                use std::io::Write;
                let _ = writeln!(f, "tray.exe {}", s);
        }
}

fn pid_alive(pid: u32) -> bool {
        unsafe {
                match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        Ok(h) => {
                                let _ = CloseHandle(h);
                                true
                        }
                        Err(_) => false,
                }
        }
}

fn refresh_icon(hwnd: HWND) {
        let (visible, is_zh) = {
                let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
                // The icon is needed while the IME is active and the user
                // opted in — or while a balloon is queued/showing (NIF_INFO
                // requires the icon to exist). Weasel does the same for its
                // deploy notifications.
                if s.balloon_until.is_some_and(|t| t <= std::time::Instant::now()) {
                        s.balloon_until = None;
                }
                let balloon_active = !s.pending_balloons.is_empty() || s.balloon_until.is_some();
                ((!s.pids.is_empty() && s.display_tray_icon) || balloon_active, s.mode_is_zh)
        };
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if visible && !st.icon_added {
                unsafe {
                        let mut data = notify_icon(hwnd);
                        data.hIcon = load_mode_icon(is_zh);
                        if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                                data.Anonymous.uVersion = 4;
                                let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
                                st.icon_added = true;
                                log("icon added");
                                let queued = std::mem::take(&mut st.pending_balloons);
                                drop(st);
                                for text in queued {
                                        show_balloon_text(hwnd, &text);
                                }
                                return;
                        } else {
                                let err = windows::Win32::Foundation::GetLastError();
                                log_error(&format!("NIM_ADD failed err={:?} icon={:?} cbSize={}", err, data.hIcon, data.cbSize));
                        }
                }
        } else if !visible && st.icon_added {
                unsafe {
                        let data = notify_icon(hwnd);
                        if Shell_NotifyIconW(NIM_DELETE, &data).as_bool() {
                                st.icon_added = false;
                                log("icon removed");
                        }
                }
        }
}

fn update_mode_icon(hwnd: HWND, is_zh: bool) {
        {
                let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                st.mode_is_zh = is_zh;
                if !st.icon_added {
                        return;
                }
        }
        unsafe {
                let mut data = notify_icon(hwnd);
                data.uFlags = NIF_ICON | NIF_TIP;
                data.hIcon = load_mode_icon(is_zh);
                let tip = if is_zh { "R-Cantonese 中" } else { "R-Cantonese A" };
                let w: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
                data.szTip[..w.len()].copy_from_slice(&w);
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
}

fn notify_icon(hwnd: HWND) -> NOTIFYICONDATAW {
        // Weasel-style: plain uID identification — no NIF_GUID. The GUID slot
        // appears to be what makes Shell_NotifyIcon return E_FAIL here.
        let mut data = NOTIFYICONDATAW::default();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_ICON_ID;
        data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        data.uCallbackMessage = WM_TRAY_ICON;
        let tip = "R-Cantonese";
        let w: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
        data.szTip[..w.len()].copy_from_slice(&w);
        data
}

fn load_mode_icon(is_zh: bool) -> HICON {
        let id: u16 = if is_zh { 21 } else { 23 };
        unsafe {
                let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                LoadImageW(
                        Some(instance.into()),
                        PCWSTR(id as usize as *const u16),
                        IMAGE_ICON,
                        0,
                        0,
                        LR_DEFAULTCOLOR | LR_DEFAULTSIZE,
                )
                .map(|h| HICON(h.0))
                .unwrap_or_default()
        }
}

fn is_chinese_ui() -> bool {
        let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        (lang & 0xff) == 0x04
}

/// One-time hint queued on the very first launch — the balloon is delivered
/// the first time the icon appears in the notification area. Tells users the
/// icon lives in the overflow and can be dragged out (or enabled via
/// Settings → Taskbar → system tray icons).
fn queue_first_run_hint() {
        let dir = std::env::var("LOCALAPPDATA")
                .map(|d| std::path::PathBuf::from(d).join("RCantonese"))
                .unwrap_or_default();
        let marker = dir.join("tray-hint-shown");
        if marker.exists() || std::fs::create_dir_all(&dir).is_err() {
                return;
        }
        if std::fs::write(&marker, b"").is_err() {
                return;
        }
        let text = if is_chinese_ui() {
                "圖標喺輸入法圖標隔籬 — 左撳切換中/英，右撳開設定"
        } else {
                "The icon sits beside the input-mode indicator — left-click toggles 中/A, right-click opens settings"
        };
        STATE.lock().unwrap_or_else(|e| e.into_inner()).pending_balloons.push(text.to_string());
}

/// Balloon with arbitrary text — sent via WM_COPYDATA (errors etc.).
fn show_balloon_text(hwnd: HWND, text: &str) {
        unsafe {
                let mut data = notify_icon(hwnd);
                data.uFlags = NIF_INFO | NIF_ICON | NIF_TIP;
                data.hIcon = load_mode_icon(STATE.lock().map(|s| s.mode_is_zh).unwrap_or(true));
                let t: Vec<u16> = "R-Cantonese".encode_utf16().chain(std::iter::once(0)).collect();
                data.szInfoTitle[..t.len()].copy_from_slice(&t);
                let b: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let n = b.len().min(data.szInfo.len());
                data.szInfo[..n].copy_from_slice(&b[..n]);
                data.Anonymous.uTimeout = 3000;
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
        mark_balloon_shown();
}

/// Keep the icon alive long enough for the balloon to be seen — the next
/// refresh_icon after the deadline hides it again when the user hasn't
/// opted into a permanent tray icon.
fn mark_balloon_shown() {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if !st.display_tray_icon {
                st.balloon_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
        }
}

fn show_balloon(hwnd: HWND, kind: usize) {
        let zh = is_chinese_ui();
        let (title, text) = match kind {
                1 => if zh { ("R-Cantonese", "設定已套用") } else { ("R-Cantonese", "Settings applied") },
                2 => if zh { ("R-Cantonese", "設定已重新載入") } else { ("R-Cantonese", "Settings reloaded") },
                3 => if zh { ("R-Cantonese", "學習記錄已清除") } else { ("R-Cantonese", "Learning data cleared") },
                _ => return,
        };
        // Balloons need the icon — if it's not up yet, queue for later.
        {
                let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                if !st.icon_added {
                        st.pending_balloons.push(text.to_string());
                        drop(st);
                        refresh_icon(hwnd);
                        return;
                }
        }
        unsafe {
                let mut data = notify_icon(hwnd);
                data.uFlags = NIF_INFO | NIF_ICON | NIF_TIP;
                data.hIcon = load_mode_icon(STATE.lock().map(|s| s.mode_is_zh).unwrap_or(true));
                let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                data.szInfoTitle[..t.len()].copy_from_slice(&t);
                let b: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                data.szInfo[..b.len()].copy_from_slice(&b);
                data.Anonymous.uTimeout = 3000;
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
        mark_balloon_shown();
}

/// Post a message to every IME host window; returns how many were found.
fn broadcast_to_hosts(msg: u32, wparam: WPARAM, lparam: LPARAM) -> usize {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let hwnds = &mut *(lparam.0 as *mut Vec<isize>);
                let mut class = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                if String::from_utf16_lossy(&class[..n]) == "RCantoneseTrayHostWnd" {
                        hwnds.push(hwnd.0 as isize);
                }
                BOOL(1)
        }
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
                let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<isize> as isize));
                for h in &hwnds {
                        let _ = PostMessageW(Some(HWND(*h as *mut std::ffi::c_void)), msg, wparam, lparam);
                }
        }
        hwnds.len()
}

/// The RCantoneseTrayHostWnd window belonging to `pid`, if any.
fn find_host_for_pid(pid: u32) -> Option<HWND> {
        unsafe extern "system" fn find(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let (target, out) = &mut *(lparam.0 as *mut (u32, isize));
                let mut class = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                if String::from_utf16_lossy(&class[..n]) == "RCantoneseTrayHostWnd" {
                        let mut owner = 0u32;
                        GetWindowThreadProcessId(hwnd, Some(&mut owner));
                        if owner == *target {
                                *out = hwnd.0 as isize;
                                return BOOL(0); // stop
                        }
                }
                BOOL(1)
        }
        let mut found = (pid, 0isize);
        unsafe {
                let _ = EnumWindows(Some(find), LPARAM(&mut found as *mut _ as isize));
        }
        if found.1 == 0 {
                None
        } else {
                Some(HWND(found.1 as *mut std::ffi::c_void))
        }
}

/// Minimal local menu for when no IME host window exists — just the
/// config-center entry.
fn show_fallback_menu(hwnd: HWND) {
        unsafe {
                let menu = CreatePopupMenu().unwrap_or_default();
                if menu.is_invalid() {
                        return;
                }
                let label = if is_chinese_ui() { "配置中心…" } else { "Settings Center…" };
                let w: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = AppendMenuW(menu, MF_STRING, IDM_CONFIG_CENTER as usize, PCWSTR(w.as_ptr()));
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                        pt.x,
                        pt.y,
                        Some(0),
                        hwnd,
                        None,
                );
                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                let _ = DestroyMenu(menu);
                if cmd.0 == IDM_CONFIG_CENTER as i32 {
                        launch_config_center();
                }
        }
}

fn launch_config_center() {
        unsafe {
                let mut path = [0u16; 260];
                let n = GetModuleFileNameW(None, &mut path) as usize;
                let exe = String::from_utf16_lossy(&path[..n]);
                let dir = exe.rsplit_once('\\').map(|(d, _)| d.to_string()).unwrap_or_default();
                let cc = format!("{}\\config-center.exe", dir);
                if !std::path::Path::new(&cc).exists() {
                        log_error("config-center.exe not found");
                        return;
                }
                let w: Vec<u16> = cc.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = ShellExecuteW(None, w!("open"), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        WM_TRAY_ICON => {
                                // Version-4 callback: lParam LOWORD = mouse
                                // event, HIWORD = icon id — mask it off.
                                match (lparam.0 as u32) & 0xFFFF {
                                        x if x == WM_RBUTTONUP => {
                                                // Route to the most recent IME
                                                // host — not a broadcast, since
                                                // only it can pop the menu.
                                                // The shell callback grants us
                                                // foreground rights; pass them on
                                                // so the host can foreground its
                                                // window for TrackPopupMenu.
                                                let _ = AllowSetForegroundWindow(ASFW_ANY);
                                                let target = {
                                                        let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                                                        if st.pids.contains(&st.last_shown) && st.last_shown != 0 {
                                                                st.last_shown
                                                        } else {
                                                                st.pids.iter().next().copied().unwrap_or(0)
                                                        }
                                                };
                                                let host = if target != 0 { find_host_for_pid(target) } else { None };
                                                match host {
                                                        Some(h) => {
                                                                let _ = PostMessageW(Some(h), WM_TRAY_MENU, WPARAM(target as usize), LPARAM(0));
                                                        }
                                                        None => show_fallback_menu(hwnd),
                                                }
                                        }
                                        x if x == WM_LBUTTONUP => {
                                                broadcast_to_hosts(WM_TRAY_TOGGLE, WPARAM(0), LPARAM(0));
                                        }
                                        _ => {}
                                }
                                LRESULT(0)
                        }
                        WM_TRAY_SHOW => {
                                let pid = wparam.0 as u32;
                                let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                                st.pids.insert(pid);
                                st.last_shown = pid;
                                drop(st);
                                refresh_icon(hwnd);
                                LRESULT(0)
                        }
                        WM_TRAY_HIDE => {
                                let pid = wparam.0 as u32;
                                STATE.lock().unwrap_or_else(|e| e.into_inner()).pids.remove(&pid);
                                refresh_icon(hwnd);
                                LRESULT(0)
                        }
                        WM_TRAY_SETMODE => {
                                update_mode_icon(hwnd, wparam.0 == 1);
                                LRESULT(0)
                        }
                        WM_TRAY_NOTIFY => {
                                // Settings may have changed (apply broadcast
                                // precedes this) — re-read the icon flag.
                                STATE.lock().unwrap_or_else(|e| e.into_inner()).display_tray_icon =
                                        read_display_tray_icon();
                                show_balloon(hwnd, wparam.0);
                                LRESULT(0)
                        }
                        WM_COPYDATA => {
                                // Free-text balloon — COPYDATASTRUCT.lpData
                                // holds a UTF-16 string from the sender.
                                let cds = lparam.0 as *const COPYDATASTRUCT;
                                if !cds.is_null() {
                                        let cds = &*cds;
                                        if !cds.lpData.is_null() && cds.cbData >= 2 {
                                                let slice = std::slice::from_raw_parts(
                                                        cds.lpData as *const u16,
                                                        (cds.cbData / 2) as usize,
                                                );
                                                let text = String::from_utf16_lossy(slice)
                                                        .trim_end_matches('\0')
                                                        .to_string();
                                                if !text.is_empty() {
                                                        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                                                        if st.icon_added {
                                                                drop(st);
                                                                show_balloon_text(hwnd, &text);
                                                        } else {
                                                                // Icon not up yet — queue it; refresh_icon
                                                                // adds the icon (balloon pending) then
                                                                // flushes the queue.
                                                                st.pending_balloons.push(text);
                                                                drop(st);
                                                                refresh_icon(hwnd);
                                                        }
                                                }
                                        }
                                }
                                LRESULT(1)
                        }
                        WM_TRAY_PING => LRESULT(TRAY_MAGIC),
                        WM_TRAY_UPDATE => {
                                STATE.lock().unwrap_or_else(|e| e.into_inner()).display_tray_icon =
                                        read_display_tray_icon();
                                refresh_icon(hwnd);
                                LRESULT(0)
                        }
                        WM_TRAY_QUIT => {
                                let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                                if st.icon_added {
                                        drop(st);
                                        let data = notify_icon(hwnd);
                                        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                                }
                                let _ = DestroyWindow(hwnd);
                                PostQuitMessage(0);
                                LRESULT(0)
                        }
                        WM_TIMER => {
                                if wparam.0 == PRUNE_TIMER_ID {
                                        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                                        let before = st.pids.len();
                                        st.pids.retain(|&pid| pid_alive(pid));
                                        if st.pids.len() != before {
                                                log(&format!("pruned {} dead pid(s)", before - st.pids.len()));
                                        }
                                        drop(st);
                                        refresh_icon(hwnd);
                                }
                                LRESULT(0)
                        }
                        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
        }
}
