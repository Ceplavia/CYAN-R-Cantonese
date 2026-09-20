// Session tray icon, Weasel-style: the icon lives in r-cantonese-tray.exe —
// a standalone process, never inside an injected process. This module only
// spawns it on demand and posts state messages to its window.
//
// Visibility protocol: each process posts WM_TRAY_SHOW (its pid) while the
// foreground thread uses our IME and WM_TRAY_HIDE when it stops. The tray
// exe keeps a pid set — the icon shows while the set is non-empty and dead
// pids are pruned on a timer, so process death can never zombie the icon.
//
// Clicks travel the other way: the tray exe posts WM_TRAY_TOGGLE /
// WM_TRAY_MENU to every host window (RCantoneseTrayHostWnd, one per
// process); the focused one answers. Settings broadcasts (reload, notify,
// clear-memory) go to all hosts.

#![allow(dead_code)]

use std::sync::{Mutex, Weak};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::TextServices::{CLSID_TF_ThreadMgr, ITfThreadMgr};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::globals;
use crate::processor::Processor;

const WM_TRAY_SETMODE: u32 = WM_APP + 41; // wParam: 1 = zh, 0 = abc
const WM_TRAY_QUIT: u32 = WM_APP + 42; // worker: quit (stale in-process workers)
const WM_TRAY_TOGGLE: u32 = WM_APP + 43; // host windows: toggle zh/abc
const WM_TRAY_MENU: u32 = WM_APP + 44; // host windows: show settings menu
const WM_TRAY_RELOAD: u32 = WM_APP + 45; // host windows: reload settings.toml
const WM_TRAY_NOTIFY: u32 = WM_APP + 46; // host windows: balloon (wParam: 1=applied, 2=reloaded, 3=memory cleared)
const WM_TRAY_CLEARMEM: u32 = WM_APP + 47; // host windows: delete all learned words
const WM_TRAY_PING: u32 = WM_APP + 48; // liveness probe — returns TRAY_MAGIC
const WM_TRAY_SHOW: u32 = WM_APP + 50; // wParam = pid — IME active+foreground
const WM_TRAY_HIDE: u32 = WM_APP + 51; // wParam = pid — IME no longer current
const WM_LANGBAR_REFRESH: u32 = WM_APP + 52; // host windows: deferred langbar icon refresh
const TRAY_MAGIC: isize = 0x5243; // 'RC'

const TRAY_WND_CLASS: PCWSTR = w!("RCantoneseTrayIconWnd");
const HOST_WND_CLASS: PCWSTR = w!("RCantoneseTrayHostWnd");

static TRAY: Mutex<TrayState> = Mutex::new(TrayState::new());

// HWND/HANDLE are raw pointers (not Send) — store as isize so the state can
// live in a static Mutex.
struct TrayState {
        host_hwnd: isize,
        shown_mode_is_zh: bool,
        shutting_down: bool,
        has_thread_focus: bool,
}

impl TrayState {
        const fn new() -> Self {
                Self {
                        host_hwnd: 0,
                        shown_mode_is_zh: true,
                        shutting_down: false,
                        has_thread_focus: false,
                }
        }
}

fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut std::ffi::c_void)
}

fn raw(hwnd: HWND) -> isize {
        hwnd.0 as isize
}

// ---------------------------------------------------------------------
// Public API — called from the IME's UI thread (TSF callbacks).
// ---------------------------------------------------------------------

/// Called when the IME becomes the active profile in this process. Ensures
/// the host window exists (created on the calling UI thread so clicks
/// dispatch into our own message loop), then shows the icon when this
/// thread is the foreground one.
pub fn activate(processor: &Weak<Mutex<Processor>>) {
        let mut tray = TRAY.lock().unwrap_or_else(|e| e.into_inner());
        tray.shutting_down = false;
        if tray.host_hwnd == 0 {
                globals::log("tray: activate before create_host_window");
                tray.host_hwnd = raw(create_host_window(processor.clone()));
                globals::log("tray: activate after create_host_window");
        }
        // OnSetThreadFocus only fires on transitions — when the IME activates
        // on an already-focused thread (fresh app, user switches IME on) no
        // event ever arrives. GetForegroundWindow is unreliable here too —
        // the IME picker flyout can be foreground during activation — so ask
        // this thread's own GUI info whether it owns the keyboard focus.
        globals::log("tray: activate before GetGUIThreadInfo");
        tray.has_thread_focus = unsafe {
                let mut gui = GUITHREADINFO::default();
                gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
                GetGUIThreadInfo(GetCurrentThreadId(), &mut gui).is_ok() && !gui.hwndFocus.is_invalid()
        };
        globals::log("tray: activate after GetGUIThreadInfo");
        globals::log(&format!("tray: activate focus={}", tray.has_thread_focus));
        if tray.has_thread_focus {
                drop(tray);
                show_tray_icon();
        }
}

/// Called on deactivation: hides the icon for this process and tears the
/// host window down.
pub fn deactivate() {
        let mut tray = TRAY.lock().unwrap_or_else(|e| e.into_inner());
        tray.shutting_down = true;
        tray.has_thread_focus = false;
        if tray.host_hwnd != 0 {
                unsafe {
                        let _ = DestroyWindow(hwnd(tray.host_hwnd));
                }
                tray.host_hwnd = 0;
        }
        drop(tray);
        hide_tray_icon();
}

/// Called from ITfThreadFocusSink::OnSetThreadFocus — this thread's IME
/// instance is now the foreground input method, so the icon should show.
pub fn thread_focus_gained() {
        let mut tray = TRAY.lock().unwrap_or_else(|e| e.into_inner());
        tray.has_thread_focus = true;
        globals::log("tray: focus gained");
        drop(tray);
        show_tray_icon();
}

/// Called from ITfThreadFocusSink::OnKillThreadFocus — the foreground moved
/// to another thread. Hide immediately: if that thread also uses our IME its
/// own OnSetThreadFocus re-shows the icon; if it uses another IME the icon
/// must go (it's a "current IME" indicator). Tray clicks don't move keyboard
/// focus, so interacting with the icon never triggers this.
pub fn thread_focus_lost() {
        let mut tray = TRAY.lock().unwrap_or_else(|e| e.into_inner());
        tray.has_thread_focus = false;
        globals::log("tray: focus lost");
        drop(tray);
        hide_tray_icon();
}

/// Called from the langbar's compartment sink OnChange — which fires inside
/// ITfCompartment::SetValue's synchronous broadcast. Touching TSF there
/// (OnUpdate → GetIcon → GetValue) deadlocks some apps, so the work is
/// deferred to a per-thread message-only window: it runs back on the same
/// thread (COM objects stay in their apartment) once SetValue returns.
pub fn post_langbar_refresh(item_ptr: usize) {
        let target = REFRESH_HWND.with(|h| {
                if h.get() == 0 {
                        h.set(raw(create_refresh_window()));
                }
                h.get()
        });
        if target != 0 {
                unsafe {
                        let _ = PostMessageW(Some(hwnd(target)), WM_LANGBAR_REFRESH, WPARAM(item_ptr), LPARAM(0));
                }
        }
}

thread_local! {
        static REFRESH_HWND: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

const REFRESH_WND_CLASS: PCWSTR = w!("RCantoneseRefreshWnd");

fn create_refresh_window() -> HWND {
        unsafe {
                let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                let mut wc = WNDCLASSEXW::default();
                wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                wc.lpfnWndProc = Some(refresh_wnd_proc);
                wc.hInstance = instance;
                wc.lpszClassName = REFRESH_WND_CLASS;
                let _ = RegisterClassExW(&wc);
                CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        REFRESH_WND_CLASS,
                        PCWSTR::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        Some(HWND_MESSAGE),
                        None,
                        Some(instance),
                        None,
                )
                .unwrap_or_default()
        }
}

unsafe extern "system" fn refresh_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                if msg == WM_LANGBAR_REFRESH {
                        return globals::guarded_value("langbar refresh", LRESULT(0), || {
                                crate::langbar::deferred_refresh(wparam.0);
                                LRESULT(0)
                        });
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
        }
}

/// Update the tray icon's zh/abc state. Finds the tray window by class and
/// posts to it — works from any process.
pub fn update_mode(is_zh: bool) {
        {
                let mut tray = TRAY.lock().unwrap_or_else(|e| e.into_inner());
                tray.shown_mode_is_zh = is_zh;
                if tray.host_hwnd != 0 && tray.has_thread_focus {
                        // Foreground IME host — make sure the icon exists.
                        drop(tray);
                        show_tray_icon();
                }
        }
        post_to_tray(WM_TRAY_SETMODE, WPARAM(is_zh as usize), LPARAM(0));
}

// ---------------------------------------------------------------------
// Tray exe lifecycle + messaging.
// ---------------------------------------------------------------------

/// Post a message to the tray exe's window if it exists.
fn post_to_tray(msg: u32, wparam: WPARAM, lparam: LPARAM) {
        if let Some(w) = find_tray_window() {
                unsafe {
                        let _ = PostMessageW(Some(w), msg, wparam, lparam);
                }
        }
}

/// Find a tray window that is genuinely owned by r-cantonese-tray.exe.
/// EnumWindows over FindWindow: stale in-process workers from older injected
/// builds share the class name, and FindWindow might return one of them.
fn find_tray_window() -> Option<HWND> {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let hwnds = &mut *(lparam.0 as *mut Vec<isize>);
                let mut class = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                if n > 0 && String::from_utf16_lossy(&class[..n]) == "RCantoneseTrayIconWnd" {
                        hwnds.push(hwnd.0 as isize);
                }
                BOOL(1)
        }
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
                let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<isize> as isize));
        }
        let mut stale = Vec::new();
        let mut found = None;
        for h in hwnds {
                let w = hwnd(h);
                let mut pid = 0u32;
                unsafe {
                        GetWindowThreadProcessId(w, Some(&mut pid));
                }
                if process_name(pid).eq_ignore_ascii_case("r-cantonese-tray.exe") {
                        found = Some(w);
                } else {
                        stale.push(w);
                }
        }
        // Stale in-process workers: tell them to quit — they can never host
        // the icon properly and they shadow the real tray window.
        for w in stale {
                let mut pid = 0u32;
                unsafe {
                        GetWindowThreadProcessId(w, Some(&mut pid));
                }
                globals::log_error(&format!("tray: stale in-process worker pid={} proc={} — quitting", pid, process_name(pid)));
                unsafe {
                        let _ = PostMessageW(Some(w), WM_TRAY_QUIT, WPARAM(0), LPARAM(0));
                }
        }
        found
}

/// Mark this process's IME as the current input method. If the tray exe
/// isn't up yet we spawn it on a background thread — never block the host
/// app's UI thread waiting for it.
fn show_tray_icon() {
        let my_pid = unsafe { GetCurrentProcessId() };
        if find_tray_window().is_some() {
                post_to_tray(WM_TRAY_SHOW, WPARAM(my_pid as usize), LPARAM(0));
                let is_zh = TRAY.lock().unwrap_or_else(|e| e.into_inner()).shown_mode_is_zh;
                post_to_tray(WM_TRAY_SETMODE, WPARAM(is_zh as usize), LPARAM(0));
                return;
        }
        std::thread::spawn(move || {
                if !spawn_tray_exe() {
                        return;
                }
                // Wait for the worker's window, then post the show.
                for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        if find_tray_window().is_some() {
                                post_to_tray(WM_TRAY_SHOW, WPARAM(my_pid as usize), LPARAM(0));
                                let is_zh = TRAY.lock().unwrap_or_else(|e| e.into_inner()).shown_mode_is_zh;
                                post_to_tray(WM_TRAY_SETMODE, WPARAM(is_zh as usize), LPARAM(0));
                                return;
                        }
                }
                globals::log_error("tray: tray.exe window never appeared");
        });
}

fn hide_tray_icon() {
        let my_pid = unsafe { GetCurrentProcessId() };
        post_to_tray(WM_TRAY_HIDE, WPARAM(my_pid as usize), LPARAM(0));
}

/// Spawn r-cantonese-tray.exe from beside the DLL. Caller is a background
/// thread — the IME's UI thread never blocks on this.
fn spawn_tray_exe() -> bool {
        let Some(exe) = tray_exe_path() else {
                globals::log_error("tray: r-cantonese-tray.exe path unavailable");
                return false;
        };
        if !std::path::Path::new(&exe).exists() {
                globals::log_error(&format!("tray: {} missing", exe));
                return false;
        }
        let mut cmd: Vec<u16> = format!("\"{}\"", exe).encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
                let mut si = windows::Win32::System::Threading::STARTUPINFOW::default();
                si.cb = std::mem::size_of::<windows::Win32::System::Threading::STARTUPINFOW>() as u32;
                let mut pi = windows::Win32::System::Threading::PROCESS_INFORMATION::default();
                let ok = windows::Win32::System::Threading::CreateProcessW(
                        PCWSTR::null(),
                        Some(windows::core::PWSTR(cmd.as_mut_ptr())),
                        None,
                        None,
                        false,
                        windows::Win32::System::Threading::CREATE_NO_WINDOW,
                        None,
                        PCWSTR::null(),
                        &mut si,
                        &mut pi,
                );
                if let Ok(()) = ok {
                        let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
                        let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
                        globals::log("tray: spawned r-cantonese-tray.exe");
                        true
                } else {
                        globals::log_error("tray: CreateProcess failed");
                        false
                }
        }
}

/// Best-effort image name for a pid ("notepad.exe"), for diagnostics and for
/// telling a real tray.exe window apart from a stale in-process worker.
fn process_name(pid: u32) -> String {
        unsafe {
                let Ok(h) = windows::Win32::System::Threading::OpenProcess(
                        windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        pid,
                ) else {
                        return format!("pid:{pid}");
                };
                let mut buf = [0u16; 260];
                let mut len = buf.len() as u32;
                let name = if windows::Win32::System::Threading::QueryFullProcessImageNameW(
                        h,
                        windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                        windows::core::PWSTR(buf.as_mut_ptr()),
                        &mut len,
                )
                .is_ok()
                {
                        String::from_utf16_lossy(&buf[..len as usize])
                } else {
                        String::new()
                };
                let _ = windows::Win32::Foundation::CloseHandle(h);
                name.rsplit('\\').next().unwrap_or("?").to_string()
        }
}

/// r-cantonese-tray.exe sits beside the registered DLL.
fn tray_exe_path() -> Option<String> {
        unsafe {
                let mut buf = [0u16; 260];
                let n = GetModuleFileNameW(Some(globals::dll_instance().into()), &mut buf) as usize;
                if n == 0 {
                        return None;
                }
                let dll = String::from_utf16_lossy(&buf[..n]);
                dll.rsplit_once('\\').map(|(dir, _)| format!("{}\\r-cantonese-tray.exe", dir))
        }
}

// ---------------------------------------------------------------------
// Host window — one per process, created on the IME's UI thread. Receives
// click/menu forwards from the tray exe and from local broadcasts.
// ---------------------------------------------------------------------

fn create_host_window(processor: Weak<Mutex<Processor>>) -> HWND {
        unsafe {
                let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                let mut wc = WNDCLASSEXW::default();
                wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                wc.lpfnWndProc = Some(host_wnd_proc);
                wc.hInstance = instance;
                wc.lpszClassName = HOST_WND_CLASS;
                let _ = RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        HOST_WND_CLASS,
                        PCWSTR::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(instance),
                        None,
                );
                let hwnd = hwnd.unwrap_or_default();
                if !hwnd.is_invalid() {
                        let weak = Box::new(processor);
                        // `as _`: SetWindowLongPtrW takes isize on x64 but
                        // i32 on x86.
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(weak) as _);
                } else {
                        globals::log_error("tray: host window creation failed — tray menu/clicks unavailable");
                }
                hwnd
        }
}

fn host_processor(hwnd: HWND) -> Option<std::sync::Arc<Mutex<Processor>>> {
        unsafe {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Weak<Mutex<Processor>>;
                if ptr.is_null() {
                        return None;
                }
                (*ptr).upgrade()
        }
}

unsafe extern "system" fn host_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        WM_TRAY_TOGGLE => {
                                globals::guarded_value("tray toggle", LRESULT(0), || {
                                        if let Some(processor) = host_processor(hwnd) {
                                                if let Ok(thread_mgr) = windows::Win32::System::Com::CoCreateInstance::<Option<_>, ITfThreadMgr>(
                                                        &CLSID_TF_ThreadMgr,
                                                        None,
                                                        windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                                                ) {
                                                        if let Ok(mut p) = processor.lock() {
                                                                let _ = p.toggle_input_method_mode(&thread_mgr);
                                                        }
                                                }
                                        }
                                        LRESULT(0)
                                })
                        }
                        WM_TRAY_MENU => {
                                globals::guarded_value("tray menu", LRESULT(0), || {
                                        // tray.exe targets ONE host by pid —
                                        // broadcasting let every injected
                                        // process race to pop a menu.
                                        if wparam.0 as u32 != GetCurrentProcessId() {
                                                return LRESULT(0);
                                        }
                                        // The shell granted tray.exe
                                        // foreground rights and it passed them
                                        // to us — claim foreground so the popup
                                        // menu doesn't dismiss instantly.
                                        let _ = SetForegroundWindow(hwnd);
                                        if let Some(processor) = host_processor(hwnd) {
                                                let mut pt = POINT::default();
                                                let _ = GetCursorPos(&mut pt);
                                                crate::langbar::show_settings_menu_at(pt, &processor);
                                        }
                                        LRESULT(0)
                                })
                        }
                        WM_TRAY_RELOAD => {
                                // Broadcast by the config center after writing
                                // settings.toml — reload and apply.
                                globals::guarded_value("tray reload", LRESULT(0), || {
                                        if let Some(processor) = host_processor(hwnd) {
                                                if let Ok(thread_mgr) = windows::Win32::System::Com::CoCreateInstance::<Option<_>, ITfThreadMgr>(
                                                        &CLSID_TF_ThreadMgr,
                                                        None,
                                                        windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                                                ) {
                                                        if let Ok(mut p) = processor.lock() {
                                                                p.apply_persisted_settings(&thread_mgr);
                                                        }
                                                }
                                        }
                                        LRESULT(0)
                                })
                        }
                        WM_TRAY_NOTIFY => {
                                // Relay to the tray exe — post it onward so
                                // the balloon shows on the icon.
                                post_to_tray(WM_TRAY_NOTIFY, wparam, lparam);
                                LRESULT(0)
                        }
                        WM_TRAY_CLEARMEM => {
                                // Config-center "clear learning" — wipe the
                                // learned-word table in this process.
                                globals::guarded_value("tray clearmem", LRESULT(0), || {
                                        if let Some(processor) = host_processor(hwnd) {
                                                if let Ok(p) = processor.lock() {
                                                        if let Ok(mem) = p.memory.lock() {
                                                                let _ = mem.delete_all();
                                                        }
                                                }
                                        }
                                        LRESULT(0)
                                })
                        }
                        WM_NCDESTROY => {
                                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Weak<Mutex<Processor>>;
                                if !ptr.is_null() {
                                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                                        drop(Box::from_raw(ptr as *mut Weak<Mutex<Processor>>));
                                }
                                DefWindowProcW(hwnd, msg, wparam, lparam)
                        }
                        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
        }
}

// ---------------------------------------------------------------------
// Settings broadcasts — called by the config center (or any process).
// ---------------------------------------------------------------------

/// Broadcast a settings reload to every active IME host window.
pub fn broadcast_reload() {
        broadcast_to_hosts(WM_TRAY_RELOAD, WPARAM(0), LPARAM(0));
}

/// Broadcast a tray balloon request — kind: 1 = applied, 2 = reloaded,
/// 3 = learning data cleared.
pub fn broadcast_notify(kind: usize) {
        broadcast_to_hosts(WM_TRAY_NOTIFY, WPARAM(kind), LPARAM(0));
}

/// Broadcast a clear-learned-words request to every active IME host window.
pub fn broadcast_clear_memory() {
        broadcast_to_hosts(WM_TRAY_CLEARMEM, WPARAM(0), LPARAM(0));
}

fn broadcast_to_hosts(msg: u32, wparam: WPARAM, lparam: LPARAM) {
        unsafe {
                let _ = EnumWindows(
                        Some(enum_hosts_proc),
                        LPARAM(&((msg, wparam, lparam)) as *const _ as isize),
                );
        }
}

unsafe extern "system" fn enum_hosts_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
                let (msg, wparam, inner) = *(lparam.0 as *const (u32, WPARAM, LPARAM));
                let mut class = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut class) as usize;
                if len > 0 && String::from_utf16_lossy(&class[..len]) == "RCantoneseTrayHostWnd" {
                        let _ = PostMessageW(Some(hwnd), msg, wparam, inner);
                }
                BOOL(1)
        }
}
