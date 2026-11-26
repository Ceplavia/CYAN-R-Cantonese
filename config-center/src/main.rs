// R-Cantonese config center — WinUI 3 (windows-reactor) app that edits
// %LOCALAPPDATA%\RCantonese\settings.toml and broadcasts a reload to every
// running IME host window.
#![windows_subsystem = "windows"]

#[path = "stub_globals.rs"]
mod globals;
#[path = "stub_settings.rs"]
mod settings;
#[path = "stub_variants.rs"]
mod variants;

// Shared TOML parse/write with the IME — same source file, stubbed deps.
#[path = "../../rcantonese/src/config.rs"]
mod config;

use settings::*;
use windows::Win32::Foundation::*;
use windows::core::BOOL;
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_reactor::*;

const WM_TRAY_RELOAD: u32 = WM_APP + 45; // must match tray.rs
const WM_TRAY_CLEARMEM: u32 = WM_APP + 47; // must match tray.rs — delete all learned words

const CLSID_RCANTONESE: windows::core::GUID =
        windows::core::GUID::from_values(0xd2291a80, 0x84d8, 0x4641, [0x9a, 0xb2, 0xbd, 0xd1, 0x47, 0x2c, 0x84, 0x6b]);
const GUID_PROFILE: windows::core::GUID =
        windows::core::GUID::from_values(0x83955c0e, 0x2c09, 0x47a5, [0xbc, 0xf3, 0xf2, 0xb9, 0x8e, 0x11, 0xee, 0x8b]);

const LOCALE_LABELS: &[(&str, u16)] = &[("中文（香港）", 0x0c04), ("中文（簡體）", 0x0804)];

fn colorref_to_winui(colorref: u32) -> Color {
        // COLORREF 0x00BBGGRR → Color(A,R,G,B)
        let r = (colorref & 0xFF) as u8;
        let g = ((colorref >> 8) & 0xFF) as u8;
        let b = ((colorref >> 16) & 0xFF) as u8;
        Color { a: 255, r, g, b }
}

fn winui_to_colorref(color: Color) -> u32 {
        (color.b as u32) << 16 | (color.g as u32) << 8 | color.r as u32
}

// -- reload broadcast (must match tray.rs) ---------------------------------

unsafe extern "system" fn enum_hosts_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
                let (msg, wparam) = *(lparam.0 as *const (u32, WPARAM));
                let mut cls = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut cls);
                if n > 0 {
                        let name: Vec<u16> = "RCantoneseTrayHostWnd".encode_utf16().collect();
                        if cls[..n as usize] == name[..] {
                                let _ = PostMessageW(Some(hwnd), msg, wparam, LPARAM(0));
                        }
                }
                BOOL(1)
        }
}

fn broadcast(msg: u32, wparam: WPARAM) {
        unsafe {
                let _ = EnumWindows(Some(enum_hosts_proc), LPARAM(&(msg, wparam) as *const _ as isize));
        }
}

fn broadcast_reload() {
        broadcast(WM_TRAY_RELOAD, WPARAM(0));
}

/// Strip WS_MAXIMIZEBOX|WS_THICKFRAME off this process's windows — the config
/// center is a fixed-size dialog — and set the embedded jyutping.ico icon.
fn disable_window_maximize() {
        use windows::Win32::System::Threading::GetCurrentProcessId;
        unsafe extern "system" fn fix(hwnd: HWND, lparam: LPARAM) -> BOOL {
                unsafe {
                        let mut pid = 0u32;
                        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
                        if pid == lparam.0 as u32 {
                                let style = GetWindowLongW(hwnd, GWL_STYLE);
                                let stripped = style & !(WS_MAXIMIZEBOX.0 | WS_THICKFRAME.0) as i32;
                                if stripped != style {
                                        let _ = SetWindowLongW(hwnd, GWL_STYLE, stripped);
                                        let _ = SetWindowPos(
                                                hwnd,
                                                None,
                                                0,
                                                0,
                                                0,
                                                0,
                                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
                                        );
                                }
                                // Icon resource 1 — jyutping.ico embedded via
                                // config-center.rc. MAKEINTRESOURCEW(1).
                                // hInstance must be our own module — NULL
                                // makes Load* return predefined system icons.
                                let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                                        .map(|h| -> HINSTANCE { h.into() })
                                        .ok();
                                for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
                                        let hicon = LoadImageW(
                                                hinst,
                                                PCWSTR(1usize as *const u16),
                                                IMAGE_ICON,
                                                size,
                                                size,
                                                LR_DEFAULTCOLOR | LR_SHARED,
                                        );
                                        if let Ok(hicon) = hicon {
                                                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(hicon.0 as isize)));
                                        }
                                }
                        }
                        BOOL(1)
                }
        }
        unsafe {
                // The XAML window may take a moment to appear — retry a few times.
                for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        let pid = GetCurrentProcessId();
                        let _ = EnumWindows(Some(fix), LPARAM(pid as isize));
                }
        }
}

// -- locale install ---------------------------------------------------------

fn installed_dll_path() -> Option<Vec<u16>> {
        use windows::Win32::System::Registry::*;
        let mut key = HKEY::default();
        let path = windows::core::w!("CLSID\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\\InProcServer32");
        unsafe {
                if RegOpenKeyExW(HKEY_CLASSES_ROOT, path, Some(0), KEY_READ, &mut key).is_err() {
                        return None;
                }
                let mut size = 0u32;
                let ok = RegQueryValueExW(key, None, None, None, None, Some(&mut size));
                if ok.is_err() || size == 0 {
                        let _ = RegCloseKey(key);
                        return None;
                }
                let mut buf = vec![0u16; (size / 2) as usize + 1];
                let ok = RegQueryValueExW(key, None, None, None, Some(buf.as_mut_ptr() as *mut u8), Some(&mut size));
                let _ = RegCloseKey(key);
                if ok.is_err() {
                        return None;
                }
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                buf.truncate(len);
                Some(buf)
        }
}

fn install_locale(langid: u16) -> Result<(), String> {
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::TextServices::*;
        unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let mgr: ITfInputProcessorProfileMgr = CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("{:?}", e.code()))?;
                // Switch the IME's registered language: remove every existing
                // profile for our CLSID (including the legacy zh-TW one),
                // then register under the target.
                for langid in [0x0c04u16, 0x0804u16, 0x0404u16] {
                        let _ = mgr.UnregisterProfile(&CLSID_RCANTONESE, langid, &GUID_PROFILE, 0);
                }
                // UnregisterProfile often leaves the lang key in place. Tray
                // treats a leftover 0x0c04 key as "still zh-HK" and, on the
                // next login, deletes the zh-CN tip Settings is listing.
                for stale in [0x0c04u16, 0x0804u16, 0x0404u16] {
                        if stale != langid {
                                delete_profile_lang(stale);
                        }
                }
                let icon = installed_dll_path().ok_or_else(|| "搵唔到已註冊嘅 DLL 路徑".to_string())?;
                let desc: Vec<u16> = "R-Cantonese".encode_utf16().chain(std::iter::once(0)).collect();
                mgr.RegisterProfile(
                        &CLSID_RCANTONESE,
                        langid,
                        &GUID_PROFILE,
                        &desc[..desc.len() - 1],
                        &icon,
                        0,
                        windows::Win32::UI::Input::KeyboardAndMouse::HKL::default(),
                        0,
                        true,
                        0,
                )
                .map_err(|e| format!("{:?}", e.code()))?;
                write_profile_enable(langid, true);
                write_installed_langid(langid);
                Ok(())
        }
}

/// Remove `LanguageProfile\0x0000{langid}` after UnregisterProfile. The
/// elevated installer is 64-bit, so this hits the native HKLM view.
/// RegisterProfile does not update Enable when the profile already exists.
fn write_profile_enable(langid: u16, enabled: bool) {
        use windows::Win32::System::Registry::{
                HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW,
                RegSetValueExW,
        };
        let path = format!(
                "SOFTWARE\\Microsoft\\CTF\\TIP\\{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}\\LanguageProfile\\{langid:#010x}\\{{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}}\0"
        );
        let wide: Vec<u16> = path.encode_utf16().collect();
        unsafe {
                let mut key = windows::Win32::System::Registry::HKEY::default();
                if RegCreateKeyExW(
                        HKEY_LOCAL_MACHINE,
                        windows::core::PCWSTR(wide.as_ptr()),
                        Some(0),
                        windows::core::PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_WRITE | KEY_WOW64_64KEY,
                        None,
                        &mut key,
                        None,
                )
                .is_err()
                {
                        return;
                }
                let name: Vec<u16> = "Enable\0".encode_utf16().collect();
                let v = u32::from(enabled);
                let _ = RegSetValueExW(key, windows::core::PCWSTR(name.as_ptr()), Some(0), REG_DWORD, Some(&v.to_le_bytes()));
                let _ = RegCloseKey(key);
        }
}

fn delete_profile_lang(langid: u16) {
        use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RegDeleteTreeW};
        let path = format!(
                "SOFTWARE\\Microsoft\\CTF\\TIP\\{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}\\LanguageProfile\\{langid:#010x}\0"
        );
        let wide: Vec<u16> = path.encode_utf16().collect();
        unsafe {
                let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, windows::core::PCWSTR(wide.as_ptr()));
        }
}

fn write_installed_langid(langid: u16) {
        use windows::Win32::System::Registry::{
                HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, RegCloseKey,
                RegCreateKeyExW, RegSetValueExW,
        };
        let path = "SOFTWARE\\Microsoft\\CTF\\TIP\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\0";
        let wide: Vec<u16> = path.encode_utf16().collect();
        unsafe {
                let mut key = windows::Win32::System::Registry::HKEY::default();
                if RegCreateKeyExW(
                        HKEY_LOCAL_MACHINE,
                        windows::core::PCWSTR(wide.as_ptr()),
                        Some(0),
                        windows::core::PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                        None,
                        &mut key,
                        None,
                )
                .is_err()
                {
                        return;
                }
                let name: Vec<u16> = "InstalledLangId".encode_utf16().chain(std::iter::once(0)).collect();
                let v = langid as u32;
                let _ = RegSetValueExW(key, windows::core::PCWSTR(name.as_ptr()), Some(0), REG_DWORD, Some(&v.to_le_bytes()));
                let _ = RegCloseKey(key);
        }
}

// -- component ---------------------------------------------------------------

struct ConfigCenter {
        page: usize, // 0 = 外觀, 1 = 快捷鍵, 2 = 語言, 3 = 進階
        locale_index: usize,
        ui_lang: usize, // 0 = auto, 1 = English, 2 = 繁體中文
        capturing: bool,
        page_size: f64,
        font: f64,
        numfont: f64,
        commentfont: f64,
        text_color: String,
        back_color: String,
        select_color: String,
        comment_color: String,
        hotkeys: String,
        status: String,
}

impl ConfigCenter {
        fn load() -> Self {
                let s = config::load();
                Self {
                        page: 0,
                        locale_index: 0,
                        ui_lang: match s.ui_language.as_str() {
                                "en" => 1,
                                "zh" => 2,
                                _ => 0,
                        },
                        capturing: false,
                        page_size: s.candidate_page_size as f64,
                        font: s.candidate_font_size as f64,
                        numfont: s.candidate_number_font_size as f64,
                        commentfont: s.candidate_comment_font_size as f64,
                        text_color: config::color_to_hex(s.candidate_text_color),
                        back_color: config::color_to_hex(s.candidate_back_color),
                        select_color: config::color_to_hex(s.candidate_select_color),
                        comment_color: config::color_to_hex(s.candidate_comment_color),
                        hotkeys: s
                                .options_menu_keys
                                .iter()
                                .map(|&(vk, m)| config::hotkey_to_spec(vk, m))
                                .collect::<Vec<_>>()
                                .join(", "),
                        status: String::new(),
                }
        }

        /// Start from the on-disk settings — fields without a UI control
        /// (input mode, variant, form, punctuation) keep their values.
        fn collect(&self) -> ImeSettings {
                let mut s = config::load();
                s.candidate_page_size = candidate_page_size_from_raw(self.page_size as u32);
                s.candidate_font_size = candidate_font_size_from_raw(self.font as u32);
                s.candidate_number_font_size = candidate_font_size_from_raw(self.numfont as u32);
                s.candidate_comment_font_size = candidate_font_size_from_raw(self.commentfont as u32);
                if let Some(v) = config::parse_color(&self.text_color) {
                        s.candidate_text_color = v;
                }
                if let Some(v) = config::parse_color(&self.back_color) {
                        s.candidate_back_color = v;
                }
                if let Some(v) = config::parse_color(&self.select_color) {
                        s.candidate_select_color = v;
                }
                if let Some(v) = config::parse_color(&self.comment_color) {
                        s.candidate_comment_color = v;
                }
                let keys: Vec<(u32, u32)> = self
                        .hotkeys
                        .split(',')
                        .filter_map(|spec| config::parse_hotkey(spec.trim()))
                        .collect();
                if !keys.is_empty() {
                        s.options_menu_keys = keys;
                }
                s.ui_language = ["auto", "en", "zh"][self.ui_lang].to_string();
                s
        }

        /// Localized UI string — follows ui_lang, resolving "auto" through
        /// the OS display language.
        fn t(&self, en: &str, zh: &str) -> String {
                let zh_mode = match self.ui_lang {
                        1 => false,
                        2 => true,
                        _ => unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() & 0x3ff == 0x04 },
                };
                if zh_mode { zh.to_string() } else { en.to_string() }
        }
}

// ---------------------------------------------------------------------
// Hotkey capture — a low-level keyboard hook reads the next combo pressed
// on the UI thread, then fires the component callback (same thread).
// ---------------------------------------------------------------------

thread_local! {
        static CAPTURED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
        static CAPTURE_CB: std::cell::RefCell<Option<Callback<()>>> = std::cell::RefCell::new(None);
        static HOOK: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn stop_capture_hook() {
        let h = HOOK.with(|c| c.replace(0));
        if h != 0 {
                unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(
                                windows::Win32::UI::WindowsAndMessaging::HHOOK(h as *mut std::ffi::c_void),
                        );
                }
        }
}

fn start_capture_hook(cb: Callback<()>) {
        CAPTURE_CB.with(|c| *c.borrow_mut() = Some(cb));
        CAPTURED.with(|c| *c.borrow_mut() = None);
        unsafe {
                if let Ok(h) = windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExW(
                        windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL,
                        Some(ll_key_hook),
                        None,
                        0,
                ) {
                        HOOK.with(|c| c.set(h.0 as isize));
                }
        }
}

unsafe extern "system" fn ll_key_hook(
        code: i32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
        use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN};
        if code == 0 && wparam.0 as u32 == WM_KEYDOWN {
                let vk = unsafe { (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode };
                // Modifier-only presses keep waiting for the real key.
                let is_modifier = matches!(vk, 0xA0..=0xA5 | 0x5B | 0x5C | 0x10 | 0x11 | 0x12);
                if !is_modifier {
                        // Esc cancels the capture.
                        let spec = if vk == 0x1B {
                                String::new()
                        } else {
                                let mut s = String::new();
                                if unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0 {
                                        s.push_str("ctrl+");
                                }
                                if unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0 {
                                        s.push_str("shift+");
                                }
                                if unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0 {
                                        s.push_str("alt+");
                                }
                                s.push_str(&config::hotkey_to_spec(vk, 0));
                                s
                        };
                        CAPTURED.with(|c| *c.borrow_mut() = Some(spec));
                        stop_capture_hook();
                        CAPTURE_CB.with(|cb| {
                                if let Some(cb) = cb.borrow().as_ref() {
                                        cb.call(());
                                }
                        });
                        return windows::Win32::Foundation::LRESULT(1); // swallow the key
                }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[derive(Clone)]
enum Msg {
        Apply,
        Nav(Option<String>),
        ClearMemory,
        InstallLocale,
        Locale(Option<usize>),
        UiLang(Option<usize>),
        Page(Option<f64>),
        Font(Option<f64>),
        NumFont(Option<f64>),
        CommentFont(Option<f64>),
        TextColor(Color),
        BackColor(Color),
        SelectColor(Color),
        CommentColor(Color),
        ResetColors,
        Hotkeys(String),
        StartCapture,
        CaptureDone,
}

impl Component for ConfigCenter {
        type Input = ();
        type Message = Msg;

        fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
                // Fixed-size dialog — strip the maximize box and size frame so
                // the window can't be maximized/resized.
                std::thread::spawn(disable_window_maximize);
                Self::load()
        }

        fn update(&mut self, message: Msg, _context: &ComponentContext<Self>) {
                match message {
                        Msg::Apply => {
                                let s = self.collect();
                                match config::save(&s) {
                                        Ok(()) => {
                                                broadcast_reload();
                                                self.status = self.t("Settings applied.", "設定已套用。");
                                                notify_windows(&self.status);
                                        }
                                        Err(e) => {
                                                self.status = format!("{}: {e}", self.t("Save failed", "儲存失敗"));
                                                notify_windows(&self.status);
                                        }
                                }
                        }
                        Msg::InstallLocale => {
                                let langid = LOCALE_LABELS[self.locale_index.min(LOCALE_LABELS.len() - 1)].1;
                                self.status = match elevate_install_locale(langid) {
                                        Ok(child) => {
                                                // The elevated child writes HKLM
                                                // (RegisterProfile). Once it exits,
                                                // install the tip into THIS user's
                                                // input list — ILOT must not run
                                                // elevated or it lands on .DEFAULT.
                                                let wait = unsafe {
                                                        windows::Win32::System::Threading::WaitForSingleObject(child, 60_000)
                                                };
                                                unsafe {
                                                        let _ = windows::Win32::Foundation::CloseHandle(child);
                                                }
                                                if wait != windows::Win32::Foundation::WAIT_OBJECT_0 {
                                                        self.t("Timed out waiting for the elevated step.", "等待提權步驟超時。")
                                                } else {
                                                        match install_tip_for_user(langid) {
                                                                Ok(()) => self.t("Input method installed.", "輸入法語言設定檔已安裝。"),
                                                                Err(e) => format!("{}{e}", self.t("Install failed: ", "安裝失敗：")),
                                                        }
                                                }
                                        }
                                        Err(e) => format!("{}{e}", self.t("Elevation failed: ", "提權失敗：")),
                                };
                        }
                        Msg::Nav(tag) => {
                                self.page = tag.as_deref().and_then(|t| t.parse().ok()).unwrap_or(0);
                        }
                        Msg::ClearMemory => {
                                broadcast(WM_TRAY_CLEARMEM, WPARAM(0));
                                self.status = self.t("Learning records cleared.", "已清除學習記錄。");
                                notify_windows(&self.status);
                        }
                        Msg::UiLang(v) => self.ui_lang = v.unwrap_or(0),
                        Msg::Locale(v) => self.locale_index = v.unwrap_or(0),
                        Msg::Page(v) => self.page_size = v.unwrap_or(7.0),
                        Msg::Font(v) => self.font = v.unwrap_or(16.0),
                        Msg::NumFont(v) => self.numfont = v.unwrap_or(13.0),
                        Msg::CommentFont(v) => self.commentfont = v.unwrap_or(13.0),
                        Msg::TextColor(c) => self.text_color = config::color_to_hex(winui_to_colorref(c)),
                        Msg::BackColor(c) => self.back_color = config::color_to_hex(winui_to_colorref(c)),
                        Msg::SelectColor(c) => self.select_color = config::color_to_hex(winui_to_colorref(c)),
                        Msg::CommentColor(c) => self.comment_color = config::color_to_hex(winui_to_colorref(c)),
                        Msg::ResetColors => {
                                let d = ImeSettings::default();
                                self.text_color = config::color_to_hex(d.candidate_text_color);
                                self.back_color = config::color_to_hex(d.candidate_back_color);
                                self.select_color = config::color_to_hex(d.candidate_select_color);
                                self.comment_color = config::color_to_hex(d.candidate_comment_color);
                        }
                        Msg::Hotkeys(s) => self.hotkeys = s,
                        Msg::StartCapture => {
                                self.capturing = true;
                                self.status = self.t("Press a key combo… (Esc to cancel)", "撳組合鍵…（Esc 取消）");
                                start_capture_hook(_context.sender().message(Msg::CaptureDone));
                        }
                        Msg::CaptureDone => {
                                self.capturing = false;
                                stop_capture_hook();
                                let spec = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
                                if spec.is_empty() {
                                        self.status = self.t("Capture cancelled.", "已取消擷取。");
                                } else {
                                        let mut parts: Vec<String> =
                                                self.hotkeys.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                                        if !parts.contains(&spec) {
                                                parts.push(spec.clone());
                                        }
                                        self.hotkeys = parts.join(", ");
                                        self.status = format!("{}{spec}", self.t("Captured: ", "已擷取："));
                                }
                        }
                }
        }

        fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
                context.window_title(self.t("R-Cantonese Config Center", "R-Cantonese 配置中心"));
                context.window_visuals(
                        WindowVisuals::new()
                                .client_size(620.0, 520.0)
                                .constraints(WindowConstraints {
                                        min_width: Some(620.0),
                                        min_height: Some(520.0),
                                        max_width: Some(620.0),
                                        max_height: Some(520.0),
                                }),
                );
                let label = |s: &str| TextBlock::new().text(s).width(110.0).vertical_alignment(VerticalAlignment::Center);
                let row = |label_text: &str, control: View| {
                        StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(8.0)
                                .children((label(label_text), control))
                };
                let combo = |items: &[&str], index: usize, cb: Callback<Option<usize>>| {
                        ComboBox::new()
                                .items_source(items.iter().map(|s| s.to_string()))
                                .selected_index(index)
                                .on_selection_changed(cb)
                                .width(200.0)
                };
                let num = |value: f64, min: f64, max: f64, cb: Callback<Option<f64>>| {
                        NumberBox::new().value(value).minimum(min).maximum(max).on_value_changed(cb).width(120.0)
                };
                let color_row = |label_text: String, value: &str, cb: Callback<Color>| {
                        let color = config::parse_color(value)
                                .map(colorref_to_winui)
                                .unwrap_or(Color { a: 255, r: 255, g: 255, b: 255 });
                        StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(8.0)
                                .children((
                                        label(&label_text),
                                        Border::new()
                                                .width(44.0)
                                                .height(24.0)
                                                .background(Brush::Solid(color))
                                                .corner_radius(CornerRadius::uniform(4.0)),
                                        Button::new()
                                                .content(self.t("Select…", "選擇…"))
                                                .flyout_with(Flyout::rich(
                                                        ColorPicker::new()
                                                                .color(color)
                                                                .is_alpha_enabled(false)
                                                                .on_color_changed(cb),
                                                )),
                                        TextBlock::new().text(value.to_string()).vertical_alignment(VerticalAlignment::Center),
                                ))
                };

                let footer = StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(12.0)
                        .margin(Thickness::new(0.0, 16.0, 0.0, 0.0))
                        .children((
                                Button::new().on_click(context.message(Msg::Apply)).content(self.t("Apply", "套用")),
                                TextBlock::new()
                                        .text(self.status.clone())
                                        .vertical_alignment(VerticalAlignment::Center),
                        ));

                let page_content: View = match self.page {
                        0 => StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::uniform(20.0))
                                .children((
                                        row(&self.t("Candidates per page", "每頁候選字數"), num(self.page_size, 1.0, 10.0, context.callback(Msg::Page)).into()),
                                        row(&self.t("Candidate font size", "候選字號"), num(self.font, 11.0, 24.0, context.callback(Msg::Font)).into()),
                                        row(&self.t("Number font size", "編號字號"), num(self.numfont, 11.0, 24.0, context.callback(Msg::NumFont)).into()),
                                        row(&self.t("Comment font size", "註釋字號"), num(self.commentfont, 11.0, 24.0, context.callback(Msg::CommentFont)).into()),
                                        View::from(color_row(self.t("Text color", "文字顏色"), &self.text_color, context.callback(Msg::TextColor))),
                                        View::from(color_row(self.t("Background color", "背景顏色"), &self.back_color, context.callback(Msg::BackColor))),
                                        View::from(color_row(self.t("Selection color", "選取顏色"), &self.select_color, context.callback(Msg::SelectColor))),
                                        View::from(color_row(self.t("Comment color", "註釋顏色"), &self.comment_color, context.callback(Msg::CommentColor))),
                                        View::from(Button::new().on_click(context.message(Msg::ResetColors)).content(self.t("Reset colors", "重置顏色"))),
                                        View::from(footer.clone()),
                                ))
                                .into(),
                        1 => StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::uniform(20.0))
                                .children((
                                        row(
                                                &self.t("Options-menu hotkey", "選單快捷鍵"),
                                                TextBox::new()
                                                        .text(self.hotkeys.clone())
                                                        .on_text_changed(context.callback(Msg::Hotkeys))
                                                        .width(240.0)
                                                        .into(),
                                        ),
                                        View::from(
                                                Button::new()
                                                        .on_click(context.message(if self.capturing { Msg::CaptureDone } else { Msg::StartCapture }))
                                                        .content(if self.capturing {
                                                                self.t("Cancel capture", "取消擷取")
                                                        } else {
                                                                self.t("Add hotkey…", "新增快捷鍵…")
                                                        }),
                                        ),
                                        TextBlock::new()
                                                .text_wrapping(TextWrapping::Wrap)
                                                .text(if self.capturing {
                                                        self.t("Press the key combo now… (Esc to cancel)", "而家撳組合鍵…（Esc 取消）")
                                                } else {
                                                        self.t(
                                                                "Format: ctrl+`, comma-separated. Supports ctrl/shift/alt combos.",
                                                                "格式：ctrl+`，多個用逗號分隔。支援 ctrl/shift/alt 組合。",
                                                        )
                                                }),
                                        View::from(footer.clone()),
                                ))
                                .into(),
                        2 => StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::uniform(20.0))
                                .children((
                                        row(
                                                &self.t("Display language", "顯示語言"),
                                                combo(
                                                        &["Follow system 跟隨系統", "English", "繁體中文"],
                                                        self.ui_lang,
                                                        context.callback(Msg::UiLang),
                                                )
                                                .into(),
                                        ),
                                        TextBlock::new().text_wrapping(TextWrapping::Wrap).text(self.t(
                                                "Applies to the tray menu and this window. \"Follow system\" uses the OS display language.",
                                                "適用於托盤選單同本視窗。「跟隨系統」會用系統顯示語言。",
                                        )),
                                        View::from(footer.clone()),
                                ))
                                .into(),
                        3 => StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::uniform(20.0))
                                .children((
                                        StackPanel::new()
                                                .orientation(Orientation::Horizontal)
                                                .spacing(8.0)
                                                .children((
                                                        label(&self.t("Install language", "安裝語言")),
                                                        combo(
                                                                &LOCALE_LABELS.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
                                                                self.locale_index,
                                                                context.callback(Msg::Locale),
                                                        ),
                                                        Button::new().on_click(context.message(Msg::InstallLocale)).content(self.t("Install", "安裝")),
                                                )),
                                        TextBlock::new().text_wrapping(TextWrapping::Wrap).text(self.t(
                                                "Switching unregisters the old profile first. Requires administrator rights.",
                                                "切換語言會先取消註冊舊 profile，再註冊新嘅。需要管理員權限。",
                                        )),
                                        Button::new().on_click(context.message(Msg::ClearMemory)).content(self.t("Clear learning records", "清除學習記錄")),
                                        TextBlock::new().text_wrapping(TextWrapping::Wrap).text(self.t(
                                                "Deletes all learned word records (Ctrl+Shift+Delete on a candidate removes one).",
                                                "清除所有已學習嘅選字記錄（候選字上面撳 Ctrl+Shift+Delete 都可以逐個刪除）。",
                                        )),
                                        View::from(footer.clone()),
                                ))
                                .into(),
                        _ => StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::uniform(20.0))
                                .children((
                                        TextBlock::new().text("CYAN-R-Cantonese").font_size(22.0),
                                        TextBlock::new().text(concat!("v", env!("CARGO_PKG_VERSION"))),
                                        TextBlock::new().text_wrapping(TextWrapping::Wrap).text(self.t(
                                                "A Cantonese Jyutping input method for Windows, reimplemented in Rust.",
                                                "用 Rust 實現嘅 Windows 粵拼輸入法。",
                                        )),
                                        HyperlinkButton::new()
                                                .navigate_uri("https://github.com/Ceplavia/CYAN-R-Cantonese")
                                                .map(|b| b.content("Github"))
                                                .unwrap_or_else(|_| HyperlinkButton::new().content("Github")),
                                ))
                                .into(),
                };

                let nav_item = |tag: &str, text: &str| {
                        KeyedView::new(
                                tag.to_string(),
                                NavigationViewItem::new()
                                        .tag(tag)
                                        .is_selected(self.page.to_string() == tag)
                                        .selects_on_invoked(true)
                                        .slot(NavigationViewItemSlot::Content, TextBlock::new().text(text)),
                        )
                };

                NavigationView::new()
                        .pane_display_mode(NavigationViewPaneDisplayMode::Left)
                        .is_settings_visible(false)
                        .is_pane_toggle_button_visible(false)
                        .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
                        .pane_title("R-Cantonese")
                        .open_pane_length(160.0)
                        .on_selected_tag_changed(context.callback(Msg::Nav))
                        .slots([
                                SlotView::collection(
                                        NavigationViewSlot::MenuItems,
                                        [
                                                nav_item("0", &self.t("Appearance", "外觀")),
                                                nav_item("1", &self.t("Hotkeys", "快捷鍵")),
                                                nav_item("2", &self.t("Language", "語言")),
                                                nav_item("3", &self.t("Advanced", "進階")),
                                                nav_item("4", &self.t("About", "關於")),
                                        ],
                                ),
                                SlotView::new(
                                        NavigationViewSlot::Content,
                                        ScrollViewer::new().content(page_content),
                                ),
                        ])
        }
}

/// Relaunch ourselves elevated to install a TSF profile — profile
/// registration writes HKLM so it needs admin rights. Returns the child
/// process handle so the caller can wait, then install the tip into the
/// *user's* input list (ILOT must not run elevated).
fn elevate_install_locale(langid: u16) -> Result<windows::Win32::Foundation::HANDLE, String> {
        use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
        use windows::Win32::UI::Shell::*;
        unsafe {
                let mut path = vec![0u16; 512];
                let len = GetModuleFileNameW(None, &mut path) as usize;
                if len == 0 || len >= path.len() {
                        return Err("GetModuleFileNameW failed".into());
                }
                path.truncate(len + 1);
                let exe = PCWSTR::from_raw(path.as_ptr());
                let params: Vec<u16> = format!("--install-locale {langid}")
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                let verb: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
                let mut info = SHELLEXECUTEINFOW {
                        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
                        hwnd: HWND::default(),
                        lpVerb: PCWSTR(verb.as_ptr()),
                        lpFile: exe,
                        lpParameters: PCWSTR(params.as_ptr()),
                        nShow: SW_HIDE.0,
                        ..Default::default()
                };
                ShellExecuteExW(&mut info).map_err(|e| format!("{:?}", e.code()))?;
                if info.hProcess.is_invalid() {
                        return Err("no child handle".into());
                }
                Ok(info.hProcess)
        }
}

// windows-link raw-dylib import — no import lib / GetProcAddress needed.
windows_link::link!("input.dll" "system" fn InstallLayoutOrTip(psz: PCWSTR, flags: u32) -> BOOL);

/// InstallLayoutOrTip for our tip under `langid` — must run in the user's
/// own context (this process is asInvoker, so it is). Also forces the
/// language into the user's language list when it's missing, otherwise the
/// tip has nothing to attach to and never appears in the picker.
fn install_tip_for_user(langid: u16) -> Result<(), String> {
        unsafe {
                // If the language itself isn't in the user's language list yet,
                // install its stock keyboard first — the tip has nothing to
                // attach to otherwise.
                if !user_has_language(langid) {
                        let kb = format!("{langid:04X}:00000409");
                        let wide: Vec<u16> = kb.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = InstallLayoutOrTip(PCWSTR(wide.as_ptr()), 0);
                }
        }
        let tip = format!("{langid:04X}:{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}{{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}}");
        let wide: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
                if !InstallLayoutOrTip(PCWSTR(wide.as_ptr()), 0).as_bool() {
                        return Err("InstallLayoutOrTip failed".into());
                }
        }
        if langid == 0x0c04 {
                drop_stale_hk_tip();
        }
        sweep_tip_entries_except(langid);
        // WeaselSetup::enable_profile runs after regsvr32, in the user process.
        enable_language_profile(langid)?;
        Ok(())
}

/// WeaselSetup::enable_profile. Enables the profile for this user and marks
/// it enabled-by-default. Does not activate the session or rewrite list order.
fn enable_language_profile(langid: u16) -> Result<(), String> {
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::TextServices::{ITfInputProcessorProfiles, CLSID_TF_InputProcessorProfiles};
        unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let profiles: ITfInputProcessorProfiles = CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("EnableLanguageProfile: {:?}", e.code()))?;
                profiles
                        .EnableLanguageProfile(&CLSID_RCANTONESE, langid, &GUID_PROFILE, true)
                        .map_err(|e| format!("EnableLanguageProfile: {:?}", e.code()))?;
                profiles
                        .EnableLanguageProfileByDefault(&CLSID_RCANTONESE, langid, &GUID_PROFILE, true)
                        .map_err(|e| format!("EnableLanguageProfileByDefault: {:?}", e.code()))?;
                if langid == 0x0c04 {
                        let _ = profiles.EnableLanguageProfile(&CLSID_RCANTONESE, 0x0404, &GUID_PROFILE, false);
                }
                Ok(())
        }
}

/// Remove our tips entries (`{langid}:{clsid}{guid}` DWORDs) from every
/// `User Profile\<lang>` key except the one for `langid` — otherwise each
/// locale switch leaves a R-Cantonese entry behind in the picker.
fn sweep_tip_entries_except(langid: u16) {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        let keep: &[&str] = match langid {
                0x0c04 => &["zh-hant-hk", "zh-hk"],
                0x0804 => &["zh-hans-cn", "zh-cn"],
                0x0404 => &["zh-hant-tw", "zh-tw"],
                _ => return,
        };
        const TIP_SUFFIX: &str = "{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";
        unsafe {
                let root_wide: Vec<u16> = "Control Panel\\International\\User Profile"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                let mut root = HKEY::default();
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(root_wide.as_ptr()), Some(0), KEY_READ, &mut root) != ERROR_SUCCESS {
                        return;
                }
                let mut i = 0u32;
                let mut langs = Vec::new();
                loop {
                        let mut buf = [0u16; 128];
                        let mut len = buf.len() as u32;
                        if RegEnumKeyExW(root, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                != ERROR_SUCCESS
                        {
                                break;
                        }
                        langs.push(String::from_utf16_lossy(&buf[..len as usize]));
                        i += 1;
                }
                let _ = RegCloseKey(root);
                for lang in langs {
                        if keep.iter().any(|k| lang.eq_ignore_ascii_case(k)) {
                                continue;
                        }
                        let path = format!("Control Panel\\International\\User Profile\\{lang}\0");
                        let wide: Vec<u16> = path.encode_utf16().collect();
                        let mut sub = HKEY::default();
                        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut sub)
                                != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let mut j = 0u32;
                        let mut doomed = Vec::new();
                        loop {
                                let mut nbuf = [0u16; 256];
                                let mut nlen = nbuf.len() as u32;
                                if RegEnumValueW(sub, j, Some(PWSTR(nbuf.as_mut_ptr())), &mut nlen, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                let name = String::from_utf16_lossy(&nbuf[..nlen as usize]);
                                if name.to_uppercase().ends_with(TIP_SUFFIX) {
                                        doomed.push(name);
                                }
                                j += 1;
                        }
                        for name in doomed {
                                let w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                                let _ = RegDeleteValueW(sub, PCWSTR(w.as_ptr()));
                        }
                        let _ = RegCloseKey(sub);
                }
        }
}

/// Remove the old `0404:{clsid}{guid}` value left under the Hong Kong language.
fn drop_stale_hk_tip() {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW};
        const STALE: &str = "0404:{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";
        unsafe {
                let name: Vec<u16> = STALE.encode_utf16().chain(std::iter::once(0)).collect();
                for tag in ["zh-Hant-HK", "zh-HK"] {
                        let path = format!("Control Panel\\International\\User Profile\\{tag}\0");
                        let wide: Vec<u16> = path.encode_utf16().collect();
                        let mut key = windows::Win32::System::Registry::HKEY::default();
                        if RegOpenKeyExW(HKEY_CURRENT_USER, windows::core::PCWSTR(wide.as_ptr()), Some(0), KEY_SET_VALUE, &mut key).is_err() {
                                continue;
                        }
                        let _ = RegDeleteValueW(key, windows::core::PCWSTR(name.as_ptr()));
                        let _ = RegCloseKey(key);
                }
        }
}

/// Whether the language for `langid` is in the user's profile language list
/// (HKCU\...\User Profile\Languages, REG_MULTI_SZ of BCP-47 names).
fn user_has_language(langid: u16) -> bool {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_MULTI_SZ, RegGetValueW};
        let expected: &[&str] = match langid {
                0x0c04 => &["zh-hant-hk", "zh-hk"],
                0x0804 => &["zh-hans-cn", "zh-cn"],
                0x0404 => &["zh-hant-tw", "zh-tw"],
                _ => &[],
        };
        unsafe {
                let key_path: Vec<u16> = "Control Panel\\International\\User Profile"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                let value: Vec<u16> = "Languages".encode_utf16().chain(std::iter::once(0)).collect();
                let mut size = 0u32;
                if RegGetValueW(
                        HKEY_CURRENT_USER,
                        windows::core::PCWSTR(key_path.as_ptr()),
                        windows::core::PCWSTR(value.as_ptr()),
                        RRF_RT_REG_MULTI_SZ,
                        None,
                        None,
                        Some(&mut size),
                )
                .is_err()
                        || size < 4
                {
                        return false;
                }
                let mut buf = vec![0u16; (size / 2) as usize + 1];
                if RegGetValueW(
                        HKEY_CURRENT_USER,
                        windows::core::PCWSTR(key_path.as_ptr()),
                        windows::core::PCWSTR(value.as_ptr()),
                        RRF_RT_REG_MULTI_SZ,
                        None,
                        Some(buf.as_mut_ptr() as *mut _),
                        Some(&mut size),
                )
                .is_err()
                {
                        return false;
                }
                buf.split(|&c| c == 0)
                        .map(|s| String::from_utf16_lossy(s).to_lowercase())
                        .any(|s| expected.contains(&s.as_str()))
        }
}

// 21H2 binds api-ms-win-appmodel-runtime-l1-1-5 to a host that does not
// export these symbols. The self-contained app does not call them; if a
// path does, return E_NOTIMPL instead of the system entry-point dialog.
unsafe extern "system" fn unavailable_package_dependency() -> i32 {
        0x80004001u32 as i32
}

unsafe extern "system" fn delay_failure_hook(notify: u32, _info: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
        // dliFailGetProc
        if notify == 4 {
                return unavailable_package_dependency as *mut core::ffi::c_void;
        }
        core::ptr::null_mut()
}

fn install_downlevel_hook() {
        use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};
        // delayimp.lib defines this hook as const data in .rdata. Assigning
        // to it without changing the page protection is an access violation
        // before the window exists.
        unsafe extern "C" {
                static mut __pfnDliFailureHook2: usize;
        }
        unsafe {
                let ptr = std::ptr::addr_of_mut!(__pfnDliFailureHook2);
                let mut old = PAGE_PROTECTION_FLAGS(0);
                if VirtualProtect(ptr.cast(), std::mem::size_of::<usize>(), PAGE_READWRITE, &mut old).is_ok() {
                        ptr.write(delay_failure_hook as *const () as usize);
                        let mut ignored = PAGE_PROTECTION_FLAGS(0);
                        let _ = VirtualProtect(ptr.cast(), std::mem::size_of::<usize>(), old, &mut ignored);
                }
        }
}

fn main() {
        install_downlevel_hook();
        // Elevated helper mode: config-center.exe --install-locale <langid>
        let args: Vec<String> = std::env::args().collect();
        if let Some(pos) = args.iter().position(|a| a == "--install-locale") {
                let langid: u16 = args
                        .get(pos + 1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                use windows::Win32::System::Com::*;
                use windows::Win32::UI::WindowsAndMessaging::*;
                unsafe {
                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                        let (body_text, style, code) = match install_locale(langid) {
                                Ok(()) => ("輸入法語言設定檔已安裝。".to_string(), MB_ICONINFORMATION.0, 0),
                                Err(e) => (format!("安裝失敗：{e}"), MB_ICONERROR.0, 1),
                        };
                        let title: Vec<u16> = "R-Cantonese".encode_utf16().chain(std::iter::once(0)).collect();
                        let body: Vec<u16> = body_text.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = MessageBoxW(None, PCWSTR(body.as_ptr()), PCWSTR(title.as_ptr()), MESSAGEBOX_STYLE(style));
                        std::process::exit(code);
                }
        }
        register_toast_identity();
        App::run_component::<ConfigCenter>(()).unwrap();
}

/// Windows toast needs an AppUserModelID that a Start-menu shortcut
/// already carries. No notification-area icon is involved.
const TOAST_AUMID: &str = "RCantonese.ConfigCenter";

fn register_toast_identity() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let _ = ensure_toast_shortcut();
                let wide: Vec<u16> = TOAST_AUMID.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr()));
        }
}

fn notify_windows(body: &str) {
        let _ = show_toast("R-Cantonese", body);
}

fn show_toast(title: &str, body: &str) -> windows::core::Result<()> {
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
        use windows::core::HSTRING;
        let xml = format!(
                "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
                xml_escape(title),
                xml_escape(body),
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(TOAST_AUMID))?.Show(&toast)
}

fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn ensure_toast_shortcut() -> windows::core::Result<()> {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::{E_FAIL, PROPERTYKEY};
        use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
        use windows::core::Interface;
        use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
        use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        let mut exe_buf = [0u16; 1024];
        let n = unsafe { GetModuleFileNameW(None, &mut exe_buf) } as usize;
        if n == 0 {
                return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        let exe = String::from_utf16_lossy(&exe_buf[..n]);
        let dir = exe.rsplit_once('\\').map(|(d, _)| d).unwrap_or("");
        let Some(appdata) = std::env::var_os("APPDATA") else {
                return Err(windows::core::Error::from_hresult(E_FAIL));
        };
        let folder = std::path::PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("R-Cantonese");
        std::fs::create_dir_all(&folder).map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
        let lnk = folder.join("配置中心.lnk");
        let lnk_wide: Vec<u16> = lnk.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(PCWSTR(exe_buf.as_ptr()))?;
                let dir_wide: Vec<u16> = dir.encode_utf16().chain(std::iter::once(0)).collect();
                link.SetWorkingDirectory(PCWSTR(dir_wide.as_ptr()))?;
                link.SetIconLocation(PCWSTR(exe_buf.as_ptr()), 0)?;
                let store: IPropertyStore = link.cast()?;
                let key = PROPERTYKEY {
                        fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
                        pid: 5,
                };
                let mut prop = string_propvariant(TOAST_AUMID)?;
                store.SetValue(&key, &prop)?;
                store.Commit()?;
                let _ = PropVariantClear(&mut prop);
                let file: IPersistFile = link.cast()?;
                file.Save(PCWSTR(lnk_wide.as_ptr()), true)?;
        }
        Ok(())
}

/// `VT_LPWSTR` allocated with `CoTaskMemAlloc`, which `PropVariantClear` frees.
/// `InitPropVariantFromString` is not an export of propsys.dll on every
/// supported Windows build, so a hard import dies before main.
fn string_propvariant(text: &str) -> windows::core::Result<windows::Win32::System::Com::StructuredStorage::PROPVARIANT> {
        use windows::Win32::Foundation::E_FAIL;
        use windows::Win32::System::Com::CoTaskMemAlloc;
        use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
        use windows::Win32::System::Variant::VT_LPWSTR;
        use windows::core::PWSTR;
        let chars: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let mem = unsafe { CoTaskMemAlloc(chars.len() * 2) };
        if mem.is_null() {
                return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        unsafe {
                std::ptr::copy_nonoverlapping(chars.as_ptr(), mem as *mut u16, chars.len());
                let mut prop = PROPVARIANT::default();
                let value = PROPVARIANT_0_0 {
                        vt: VT_LPWSTR,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(mem as *mut u16) },
                };
                std::ptr::write(std::ptr::addr_of_mut!(prop.Anonymous.Anonymous).cast(), value);
                Ok(prop)
        }
}
