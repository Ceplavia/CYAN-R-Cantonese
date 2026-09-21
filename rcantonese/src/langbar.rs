// Language bar button — port of LanguageBar.cpp (CLangBarItemButton) and
// SettingsMenuModel.cpp.
#![allow(dead_code)]

use std::sync::{Mutex, Weak};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::globals;
use crate::processor::Processor;
use crate::settings::*;

pub const TF_LBI_STATUS: u32 = 0x02;
pub const TF_LBI_ICON: u32 = 0x10;
// TF_LBI_STYLE_BTN_BUTTON / TF_LBI_STYLE_SHOWNINTRAY come from
// windows::Win32::UI::TextServices — do NOT redefine them here
// (SHOWNINTRAY is 0x2, not 0x00100000 = TEXTCOLORICON).

const CONNECT_E_CANNOTCONNECT: HRESULT = HRESULT(0x80040202u32 as i32);
const CONNECT_E_ADVISELIMIT: HRESULT = HRESULT(0x80040201u32 as i32);
const CONNECT_E_NOCONNECTION: HRESULT = HRESULT(0x80040200u32 as i32);

// ---------------------------------------------------------------------
// Settings menu IDs — port of SettingsMenuModel.h constants.
// ---------------------------------------------------------------------

pub mod menu_id {
        use crate::settings::{MAXIMUM_CANDIDATE_FONT_SIZE, MINIMUM_CANDIDATE_FONT_SIZE};

        pub const CANDIDATE_FONT_SIZE: u32 = 1;
        pub const CANDIDATE_FONT_SIZE_FIRST: u32 = CANDIDATE_FONT_SIZE + 1;
        pub const CANDIDATE_FONT_SIZE_LAST: u32 =
                CANDIDATE_FONT_SIZE_FIRST + MAXIMUM_CANDIDATE_FONT_SIZE - MINIMUM_CANDIDATE_FONT_SIZE;
        pub const CANDIDATE_NUMBER_FONT_SIZE: u32 = CANDIDATE_FONT_SIZE_LAST + 1;
        pub const CANDIDATE_NUMBER_FONT_SIZE_FIRST: u32 = CANDIDATE_NUMBER_FONT_SIZE + 1;
        pub const CANDIDATE_NUMBER_FONT_SIZE_LAST: u32 =
                CANDIDATE_NUMBER_FONT_SIZE_FIRST + MAXIMUM_CANDIDATE_FONT_SIZE - MINIMUM_CANDIDATE_FONT_SIZE;
        pub const CANDIDATE_COMMENT_FONT_SIZE: u32 = CANDIDATE_NUMBER_FONT_SIZE_LAST + 1;
        pub const CANDIDATE_COMMENT_FONT_SIZE_FIRST: u32 = CANDIDATE_COMMENT_FONT_SIZE + 1;
        pub const CANDIDATE_COMMENT_FONT_SIZE_LAST: u32 =
                CANDIDATE_COMMENT_FONT_SIZE_FIRST + MAXIMUM_CANDIDATE_FONT_SIZE - MINIMUM_CANDIDATE_FONT_SIZE;
        pub const CANDIDATE_PAGE_SIZE: u32 = CANDIDATE_COMMENT_FONT_SIZE_LAST + 1;
        pub const CANDIDATE_PAGE_SIZE_FIRST: u32 = CANDIDATE_PAGE_SIZE + 1;
        pub const CANDIDATE_PAGE_SIZE_LAST: u32 = CANDIDATE_PAGE_SIZE_FIRST + 9;
        pub const PUNCTUATION_FORM: u32 = CANDIDATE_PAGE_SIZE_LAST + 1;
        pub const PUNCTUATION_FORM_CANTONESE: u32 = PUNCTUATION_FORM + 1;
        pub const PUNCTUATION_FORM_ENGLISH: u32 = PUNCTUATION_FORM_CANTONESE + 1;
        pub const CHARACTER_FORM: u32 = PUNCTUATION_FORM_ENGLISH + 1;
        pub const CHARACTER_FORM_HALF_WIDTH: u32 = CHARACTER_FORM + 1;
        pub const CHARACTER_FORM_FULL_WIDTH: u32 = CHARACTER_FORM_HALF_WIDTH + 1;
        pub const CHARACTER_VARIANT: u32 = CHARACTER_FORM_FULL_WIDTH + 1;
        pub const CHARACTER_VARIANT_TRADITIONAL: u32 = CHARACTER_VARIANT + 1;
        pub const CHARACTER_VARIANT_HONGKONG: u32 = CHARACTER_VARIANT_TRADITIONAL + 1;
        pub const CHARACTER_VARIANT_TAIWAN: u32 = CHARACTER_VARIANT_HONGKONG + 1;
        pub const CHARACTER_VARIANT_SIMPLIFIED: u32 = CHARACTER_VARIANT_TAIWAN + 1;
        pub const SEPARATOR: u32 = CHARACTER_VARIANT_SIMPLIFIED + 1;
        pub const MORE_SETTINGS: u32 = SEPARATOR + 1;
}

pub fn font_size_from_id(first_id: u32, id: u32) -> u32 {
        MINIMUM_CANDIDATE_FONT_SIZE + id - first_id
}
pub fn font_size_id(first_id: u32, font_size: u32) -> u32 {
        first_id + font_size - MINIMUM_CANDIDATE_FONT_SIZE
}
pub fn page_size_id(page_size: u32) -> u32 {
        menu_id::CANDIDATE_PAGE_SIZE_FIRST + page_size - 1
}
pub fn page_size_from_id(id: u32) -> u32 {
        id - menu_id::CANDIDATE_PAGE_SIZE_FIRST + 1
}

#[derive(Clone)]
pub enum MenuItem {
        Command { id: u32, text: String, enabled: bool, checked: bool },
        Submenu { id: u32, text: String, children: Vec<MenuItem> },
        Separator,
}

/// Port of SettingsMenu::BuildSnapshot.
pub fn build_menu_snapshot(processor: &Processor) -> Vec<MenuItem> {
        let mut items = Vec::new();

        let font_items = |first: u32, current: u32| -> Vec<MenuItem> {
                (MINIMUM_CANDIDATE_FONT_SIZE..=MAXIMUM_CANDIDATE_FONT_SIZE)
                        .map(|size| MenuItem::Command {
                                id: font_size_id(first, size),
                                text: size.to_string(),
                                enabled: true,
                                checked: current == size,
                        })
                        .collect()
        };
        use crate::strings::*;
        items.push(MenuItem::Submenu {
                id: menu_id::CANDIDATE_FONT_SIZE,
                text: text_or(IDS_MENU_CANDIDATE_FONT_SIZE, "Candidate Font Size").into(),
                children: font_items(menu_id::CANDIDATE_FONT_SIZE_FIRST, processor.current_candidate_font_size()),
        });
        items.push(MenuItem::Submenu {
                id: menu_id::CANDIDATE_NUMBER_FONT_SIZE,
                text: text_or(IDS_MENU_CANDIDATE_NUMBER_FONT_SIZE, "Number Font Size").into(),
                children: font_items(menu_id::CANDIDATE_NUMBER_FONT_SIZE_FIRST, processor.current_candidate_number_font_size()),
        });
        items.push(MenuItem::Submenu {
                id: menu_id::CANDIDATE_COMMENT_FONT_SIZE,
                text: text_or(IDS_MENU_CANDIDATE_COMMENT_FONT_SIZE, "Comment Font Size").into(),
                children: font_items(menu_id::CANDIDATE_COMMENT_FONT_SIZE_FIRST, processor.current_candidate_comment_font_size()),
        });

        items.push(MenuItem::Submenu {
                id: menu_id::CANDIDATE_PAGE_SIZE,
                text: text_or(IDS_MENU_CANDIDATE_PAGE_SIZE, "Candidates per Page").into(),
                children: (1..=10)
                        .map(|size| MenuItem::Command {
                                id: page_size_id(size),
                                text: size.to_string(),
                                enabled: true,
                                checked: processor.current_candidate_page_size() == size,
                        })
                        .collect(),
        });

        let punct = processor.current_punctuation_form();
        items.push(MenuItem::Submenu {
                id: menu_id::PUNCTUATION_FORM,
                text: text_or(IDS_MENU_PUNCTUATION_FORM, "Punctuation Form").into(),
                children: vec![
                        MenuItem::Command { id: menu_id::PUNCTUATION_FORM_CANTONESE, text: text_or(IDS_MENU_PUNCTUATION_FORM_CANTONESE, "Cantonese").into(), enabled: true, checked: punct == PunctuationForm::Cantonese },
                        MenuItem::Command { id: menu_id::PUNCTUATION_FORM_ENGLISH, text: text_or(IDS_MENU_PUNCTUATION_FORM_ENGLISH, "English").into(), enabled: true, checked: punct == PunctuationForm::English },
                ],
        });

        let form = processor.current_character_form();
        items.push(MenuItem::Submenu {
                id: menu_id::CHARACTER_FORM,
                text: text_or(IDS_MENU_CHARACTER_FORM, "Half-width / Full-width").into(),
                children: vec![
                        MenuItem::Command { id: menu_id::CHARACTER_FORM_HALF_WIDTH, text: text_or(IDS_MENU_CHARACTER_FORM_HALF_WIDTH, "Half-width").into(), enabled: true, checked: form == CharacterForm::HalfWidth },
                        MenuItem::Command { id: menu_id::CHARACTER_FORM_FULL_WIDTH, text: text_or(IDS_MENU_CHARACTER_FORM_FULL_WIDTH, "Full-width").into(), enabled: true, checked: form == CharacterForm::FullWidth },
                ],
        });

        let variant = processor.current_character_variant();
        items.push(MenuItem::Submenu {
                id: menu_id::CHARACTER_VARIANT,
                text: text_or(IDS_MENU_CHARACTER_VARIANT, "Candidate Characters").into(),
                children: vec![
                        MenuItem::Command { id: menu_id::CHARACTER_VARIANT_TRADITIONAL, text: text_or(IDS_MENU_CHARACTER_VARIANT_TRADITIONAL, "Traditional").into(), enabled: true, checked: variant == crate::variants::CharacterVariant::Traditional },
                        MenuItem::Command { id: menu_id::CHARACTER_VARIANT_HONGKONG, text: text_or(IDS_MENU_CHARACTER_VARIANT_HONG_KONG, "Traditional, HK").into(), enabled: true, checked: variant == crate::variants::CharacterVariant::HongKong },
                        MenuItem::Command { id: menu_id::CHARACTER_VARIANT_TAIWAN, text: text_or(IDS_MENU_CHARACTER_VARIANT_TAIWAN, "Traditional, TW").into(), enabled: true, checked: variant == crate::variants::CharacterVariant::Taiwan },
                        MenuItem::Command { id: menu_id::CHARACTER_VARIANT_SIMPLIFIED, text: text_or(IDS_MENU_CHARACTER_VARIANT_SIMPLIFIED, "Simplified").into(), enabled: true, checked: variant == crate::variants::CharacterVariant::Simplified },
                ],
        });

        items.push(MenuItem::Separator);
        items.push(MenuItem::Command {
                id: menu_id::MORE_SETTINGS,
                text: text_or(IDS_MENU_MORE_SETTINGS, "Settings Center…").into(),
                enabled: true,
                checked: false,
        });
        items
}

/// Port of SettingsMenu::AddTfItems.
fn add_tf_items(menu: &ITfMenu, items: &[MenuItem]) -> Result<()> {
        const TF_LBMENUF_SUBMENU: u32 = 0x01;
        const TF_LBMENUF_SEPARATOR: u32 = 0x02;
        const TF_LBMENUF_RADIOCHECKED: u32 = 0x04;
        const TF_LBMENUF_GRAYED: u32 = 0x08;
        for item in items {
                match item {
                        MenuItem::Separator => {
                                let mut submenu: Option<ITfMenu> = None;
                                unsafe {
                                        menu.AddMenuItem(
                                                menu_id::SEPARATOR,
                                                TF_LBMENUF_SEPARATOR,
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                &[],
                                                &mut submenu,
                                        )?;
                                }
                        }
                        MenuItem::Command { id, text, enabled, checked } => {
                                let mut flags = 0;
                                if !enabled {
                                        flags |= TF_LBMENUF_GRAYED;
                                }
                                if *checked {
                                        flags |= TF_LBMENUF_RADIOCHECKED;
                                }
                                let wide: Vec<u16> = text.encode_utf16().collect();
                                let mut submenu: Option<ITfMenu> = None;
                                unsafe {
                                        menu.AddMenuItem(
                                                *id,
                                                flags,
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                &wide,
                                                &mut submenu,
                                        )?;
                                }
                        }
                        MenuItem::Submenu { id, text, children } => {
                                let wide: Vec<u16> = text.encode_utf16().collect();
                                let mut submenu: Option<ITfMenu> = None;
                                unsafe {
                                        menu.AddMenuItem(
                                                *id,
                                                TF_LBMENUF_SUBMENU,
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                windows::Win32::Graphics::Gdi::HBITMAP::default(),
                                                &wide,
                                                &mut submenu,
                                        )?;
                                }
                                if let Some(submenu) = submenu {
                                        add_tf_items(&submenu, children)?;
                                }
                        }
                }
        }
        Ok(())
}

// ---------------------------------------------------------------------
// Compartment event sink — port of CCompartmentEventSink.
// ---------------------------------------------------------------------

/// Run a shell-facing COM method body, converting any panic into E_FAIL so a
/// fault in our code can never unwind into explorer/ctfmon.
fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
                crate::globals::log_error("langbar: panic caught in COM method");
                Err(Error::from_hresult(E_FAIL))
        })
}

#[implement(ITfCompartmentEventSink)]
pub struct CompartmentEventSink {
        callback: Mutex<Option<Box<dyn Fn(&GUID) + Send + Sync>>>,
}

impl CompartmentEventSink {
        pub fn new(callback: impl Fn(&GUID) + Send + Sync + 'static) -> Self {
                Self {
                        callback: Mutex::new(Some(Box::new(callback))),
                }
        }

        /// Advise the sink on the compartment's own ITfSource — upstream
        /// CCompartmentEventSink::_Advise attaches to the ITfCompartment
        /// object, not the thread manager. Returns the cookie plus the
        /// compartment's ITfSource, which the caller must keep alive.
        pub fn advise(sink: &ITfCompartmentEventSink, punk: &IUnknown, guid: &GUID) -> Result<(u32, ITfSource)> {
                let mgr: ITfCompartmentMgr = punk.cast()?;
                let compartment = unsafe { mgr.GetCompartment(guid)? };
                let source: ITfSource = compartment.cast()?;
                let unknown: IUnknown = sink.clone().cast::<IUnknown>()?;
                let cookie = unsafe { source.AdviseSink(&ITfCompartmentEventSink::IID, &unknown)? };
                Ok((cookie, source))
        }

        pub fn unadvise(source: &ITfSource, cookie: u32) {
                unsafe {
                        let _ = source.UnadviseSink(cookie);
                }
        }
}

impl ITfCompartmentEventSink_Impl for CompartmentEventSink_Impl {
        fn OnChange(&self, rguid: *const GUID) -> Result<()> {
                guarded(|| {
                        if rguid.is_null() {
                                return Err(Error::from_hresult(E_INVALIDARG));
                        }
                        let guid = unsafe { *rguid };
                        if let Some(callback) = self.callback.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                                callback(&guid);
                        }
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// Deferred refresh — the compartment sink fires inside
// ITfCompartment::SetValue's synchronous broadcast, where calling back
// into TSF (OnUpdate → GetIcon → GetValue) deadlocks some apps. The sink
// only posts the item pointer to the process host window; this function
// runs later on that window's message loop, after the broadcast settles.
// ---------------------------------------------------------------------

static REFRESH_ITEMS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Runs on the host window's thread — safe to talk to TSF again.
pub fn deferred_refresh(item_ptr: usize) {
        let items = REFRESH_ITEMS.lock().unwrap_or_else(|e| e.into_inner());
        if !items.contains(&item_ptr) {
                return;
        }
        let item = unsafe { &*(item_ptr as *const LangBarItem) };
        item.notify_update(TF_LBI_ICON | TF_LBI_STATUS);
        // Mirror the new mode onto the tray icon — Caps Lock forces "en"
        // without touching the compartment, same as GetIcon does.
        if let Some(compartment) = item.compartment() {
                if let Ok(is_open) = compartment.get_bool() {
                        crate::tray::update_mode(is_open && !crate::keys::caps_lock_on());
                }
        }
}

/// Deferred settings-menu dispatch — runs on the owning UI thread once the
/// click's button-up is consumed. Same liveness contract as deferred_refresh.
pub fn deferred_settings_menu(item_ptr: usize, x: i32, y: i32) {
        let items = REFRESH_ITEMS.lock().unwrap_or_else(|e| e.into_inner());
        if !items.contains(&item_ptr) {
                return;
        }
        let item = unsafe { &*(item_ptr as *const LangBarItem) };
        let handler = item.settings_handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(weak) = handler {
                if let Some(processor) = weak.upgrade() {
                        show_settings_menu_at(POINT { x, y }, &processor);
                }
        }
}

/// Post a deferred refresh for every live langbar item — for mode changes
/// that aren't compartment events (Caps Lock), which never trigger the
/// compartment sink.
pub fn post_refresh_all() {
        let items: Vec<usize> = REFRESH_ITEMS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for ptr in items {
                crate::tray::post_langbar_refresh(ptr);
        }
}

// ---------------------------------------------------------------------
// LangBarItem — port of CLangBarItemButton.
// ---------------------------------------------------------------------

pub type ProcessorRef = std::sync::Arc<Mutex<Processor>>;

#[implement(ITfLangBarItem, ITfLangBarItemButton, ITfSource)]
pub struct LangBarItem {
        pub info: Mutex<TF_LANGBARITEMINFO>,
        tooltip: String,
        status: AtomicU32,
        sink: Mutex<Option<ITfLangBarItemSink>>,
        sink_cookie: u32,

        on_icon: u32,
        off_icon: u32,

        is_added: AtomicBool,
        is_secure_mode: bool,

        compartment: Mutex<Option<crate::compartment::Compartment>>,
        compartment_sink_cookie: Mutex<u32>,
        compartment_sink: Mutex<Option<ITfCompartmentEventSink>>,
        compartment_source: Mutex<Option<ITfSource>>,

        pub settings_handler: Mutex<Option<Weak<Mutex<Processor>>>>,
}

impl LangBarItem {
        pub fn new(
                guid: GUID,
                description: &str,
                tooltip: &str,
                on_icon: u32,
                off_icon: u32,
                is_secure_mode: bool,
        ) -> Self {
                let mut info = TF_LANGBARITEMINFO {
                        clsidService: globals::CLSID_RCANTONESE,
                        guidItem: guid,
                        // BTN_MENU tells the shell to drive the right-click
                        // menu through InitMenu/OnMenuSelect (weasel-style) —
                        // without it the shell never treats the item as a
                        // menu button and our own modal popup thread risks
                        // hanging explorer.
                        dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_BTN_MENU | TF_LBI_STYLE_SHOWNINTRAY,
                        // Weasel uses ulSort=1 for its tray button.
                        ulSort: 1,
                        szDescription: [0u16; 32],
                };
                for (i, c) in description.encode_utf16().take(31).enumerate() {
                        info.szDescription[i] = c;
                }
                globals::dll_add_ref();
                Self {
                        info: Mutex::new(info),
                        tooltip: tooltip.to_string(),
                        status: AtomicU32::new(0),
                        sink: Mutex::new(None),
                        sink_cookie: 0x5AFE,
                        on_icon,
                        off_icon,
                        is_added: AtomicBool::new(false),
                        is_secure_mode,
                        compartment: Mutex::new(None),
                        compartment_sink_cookie: Mutex::new(TF_INVALID_COOKIE),
                        compartment_sink: Mutex::new(None),
                        compartment_source: Mutex::new(None),
                        settings_handler: Mutex::new(None),
                }
        }

        /// Port of _AddItem.
        pub fn add_item(this: &ComObject<LangBarItem>, thread_mgr: &ITfThreadMgr) -> bool {
                if this.get().is_added.load(Ordering::Relaxed) {
                        return true;
                }
                match thread_mgr.cast::<ITfLangBarItemMgr>() {
                        Ok(mgr) => {
                                let item: ITfLangBarItem = this.to_interface();
                                let mut result = unsafe { mgr.AddItem(&item) };
                                if result.is_err() {
                                        // A stale item with our guid may survive a previous
                                        // activate/crash — remove it and retry once.
                                        let _ = unsafe { mgr.RemoveItem(&item) };
                                        result = unsafe { mgr.AddItem(&item) };
                                }
                                match result {
                                        Ok(()) => {
                                                crate::globals::log("langbar AddItem ok");
                                                this.get().is_added.store(true, Ordering::Relaxed);
                                                return true;
                                        }
                                        Err(e) => crate::globals::log_error(&format!("langbar AddItem failed {e:?}")),
                                }
                        }
                        Err(e) => crate::globals::log_error(&format!("langbar item mgr cast failed {e:?}")),
                }
                false
        }

        /// Unadvise the compartment event sink — port of _UnregisterCompartment.
        pub fn unadvise_compartment_sink(this: &ComObject<LangBarItem>) {
                let ptr = this.get() as *const LangBarItem as usize;
                REFRESH_ITEMS.lock().unwrap_or_else(|e| e.into_inner()).retain(|&p| p != ptr);
                let this = this.get();
                let cookie = this.compartment_sink_cookie.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let source = this.compartment_source.lock().unwrap_or_else(|e| e.into_inner()).take();
                *this.compartment_sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
                *this.compartment_sink_cookie.lock().unwrap_or_else(|e| e.into_inner()) = TF_INVALID_COOKIE;
                if let Some(source) = source {
                        if cookie != TF_INVALID_COOKIE {
                                CompartmentEventSink::unadvise(&source, cookie);
                        }
                }
        }

        pub fn remove_item(this: &ComObject<LangBarItem>, thread_mgr: &ITfThreadMgr) {
                if !this.get().is_added.load(Ordering::Relaxed) {
                        return;
                }
                if let Ok(mgr) = thread_mgr.cast::<ITfLangBarItemMgr>() {
                        let item: ITfLangBarItem = this.to_interface();
                        if unsafe { mgr.RemoveItem(&item) }.is_ok() {
                                this.get().is_added.store(false, Ordering::Relaxed);
                        }
                }
        }

        /// Port of _RegisterCompartment — advise a compartment sink that refreshes the icon.
        pub fn register_compartment(this: &ComObject<LangBarItem>, thread_mgr: &ITfThreadMgr, client_id: u32, guid: GUID) {
                let thread_unknown: IUnknown = match thread_mgr.cast() {
                        Ok(u) => u,
                        Err(_) => return,
                };
                *this.get().compartment.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(crate::compartment::Compartment::new(&thread_unknown, client_id, guid));

                // Safe while the COM object is alive — TSF holds a reference
                // through the sink, and REFRESH_ITEMS drops the pointer on
                // unadvise before the item can be freed.
                let this_ptr = this.get() as *const LangBarItem as usize;
                let sink_obj: ITfCompartmentEventSink = CompartmentEventSink::new(move |_| {
                        // Never touch TSF in here — OnChange runs inside the
                        // SetValue broadcast and re-entering deadlocks some
                        // apps. Defer the icon refresh to the host window.
                        crate::tray::post_langbar_refresh(this_ptr);
                })
                .into();
                match CompartmentEventSink::advise(&sink_obj, &thread_unknown, &guid) {
                        Ok((cookie, source)) => {
                                REFRESH_ITEMS.lock().unwrap_or_else(|e| e.into_inner()).push(this_ptr);
                                *this.get().compartment_sink_cookie.lock().unwrap_or_else(|e| e.into_inner()) = cookie;
                                *this.get().compartment_source.lock().unwrap_or_else(|e| e.into_inner()) = Some(source);
                                *this.get().compartment_sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink_obj);
                                crate::globals::log("langbar compartment sink advised");
                        }
                        Err(e) => crate::globals::log_error(&format!("langbar compartment advise failed {e:?}")),
                }
        }

        fn notify_update(&self, flags: u32) {
                if let Some(sink) = self.sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        unsafe {
                                let _ = sink.OnUpdate(flags);
                        }
                }
        }

        /// Port of SetStatus.
        pub fn set_status(&self, status: u32, is_set: bool) {
                let current = self.status.load(Ordering::Relaxed);
                let next = if is_set { current | status } else { current & !status };
                if next != current {
                        self.status.store(next, Ordering::Relaxed);
                        self.notify_update(TF_LBI_STATUS | TF_LBI_ICON);
                }
        }

        /// Refresh icon state after compartment change (open/close).
        pub fn update(&self, _is_open: bool) {
                self.notify_update(TF_LBI_ICON | TF_LBI_STATUS);
        }

        fn compartment(&self) -> Option<crate::compartment::Compartment> {
                self.compartment.lock().unwrap_or_else(|e| e.into_inner()).as_ref().cloned()
        }

        /// Dispatch a settings-menu command to the processor. Returns true if handled.
        pub fn on_menu_select(&self, wid: u32) {
                let handler = self.settings_handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let Some(weak) = handler else { return };
                let Some(processor) = weak.upgrade() else { return };
                // try_lock — called on the shell's UI thread; a blocking wait
                // here can hang explorer.
                let Ok(mut processor) = processor.try_lock() else { return };
                menu_command(&mut processor, wid);
        }
}

/// Launch config-center.exe sitting next to this DLL.
fn launch_config_center() {
        unsafe {
                let mut buf = [0u16; 512];
                let n = windows::Win32::System::LibraryLoader::GetModuleFileNameW(Some(globals::dll_instance().into()), &mut buf) as usize;
                if n == 0 {
                        return;
                }
                let mut path = String::from_utf16_lossy(&buf[..n]);
                if let Some(pos) = path.rfind('\\') {
                        path.truncate(pos + 1);
                }
                path.push_str("config-center.exe");
                if !std::path::Path::new(&path).exists() {
                        crate::globals::log_error("config-center.exe not found next to DLL");
                        return;
                }
                let mut exe: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
                let result = windows::Win32::UI::Shell::ShellExecuteW(
                        None,
                        w!("open"),
                        PCWSTR(exe.as_ptr()),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                );
                let _ = &mut exe;
                let _ = result;
        }
}

/// Port of the settings-menu command dispatch — shared by OnClick's popup
/// thread and ITfLangBarItemButton::OnMenuSelect.
fn menu_command(processor: &mut Processor, wid: u32) {
        {
                let thread_mgr: Option<ITfThreadMgr> = unsafe {
                        windows::Win32::System::Com::CoCreateInstance(
                                &CLSID_TF_ThreadMgr,
                                None,
                                windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                        )
                        .ok()
                };

                use menu_id::*;
                if (CANDIDATE_FONT_SIZE_FIRST..=CANDIDATE_FONT_SIZE_LAST).contains(&wid) {
                        processor.set_candidate_font_size(font_size_from_id(CANDIDATE_FONT_SIZE_FIRST, wid));
                        return;
                }
                if (CANDIDATE_NUMBER_FONT_SIZE_FIRST..=CANDIDATE_NUMBER_FONT_SIZE_LAST).contains(&wid) {
                        processor.set_candidate_number_font_size(font_size_from_id(CANDIDATE_NUMBER_FONT_SIZE_FIRST, wid));
                        return;
                }
                if (CANDIDATE_COMMENT_FONT_SIZE_FIRST..=CANDIDATE_COMMENT_FONT_SIZE_LAST).contains(&wid) {
                        processor.set_candidate_comment_font_size(font_size_from_id(CANDIDATE_COMMENT_FONT_SIZE_FIRST, wid));
                        return;
                }
                if (CANDIDATE_PAGE_SIZE_FIRST..=CANDIDATE_PAGE_SIZE_LAST).contains(&wid) {
                        processor.set_candidate_page_size(page_size_from_id(wid));
                        return;
                }
                match wid {
                        PUNCTUATION_FORM_CANTONESE => {
                                if let Some(tm) = &thread_mgr {
                                        processor.set_punctuation_form(PunctuationForm::Cantonese, tm);
                                }
                        }
                        PUNCTUATION_FORM_ENGLISH => {
                                if let Some(tm) = &thread_mgr {
                                        processor.set_punctuation_form(PunctuationForm::English, tm);
                                }
                        }
                        CHARACTER_FORM_HALF_WIDTH => {
                                if let Some(tm) = &thread_mgr {
                                        processor.set_character_form(CharacterForm::HalfWidth, tm);
                                }
                        }
                        CHARACTER_FORM_FULL_WIDTH => {
                                if let Some(tm) = &thread_mgr {
                                        processor.set_character_form(CharacterForm::FullWidth, tm);
                                }
                        }
                        CHARACTER_VARIANT_TRADITIONAL => {
                                processor.set_character_variant(crate::variants::CharacterVariant::Traditional);
                        }
                        CHARACTER_VARIANT_HONGKONG => {
                                processor.set_character_variant(crate::variants::CharacterVariant::HongKong);
                        }
                        CHARACTER_VARIANT_TAIWAN => {
                                processor.set_character_variant(crate::variants::CharacterVariant::Taiwan);
                        }
                        CHARACTER_VARIANT_SIMPLIFIED => {
                                processor.set_character_variant(crate::variants::CharacterVariant::Simplified);
                        }
                        MORE_SETTINGS => launch_config_center(),
                        _ => {}
                }
        }
}

impl ITfLangBarItem_Impl for LangBarItem_Impl {
        fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
                guarded(|| {
                        if pinfo.is_null() {
                                return Err(Error::from_hresult(E_INVALIDARG));
                        }
                        let mut info = self.info.lock().unwrap_or_else(|e| e.into_inner());
                        info.dwStyle |= TF_LBI_STYLE_SHOWNINTRAY;
                        unsafe {
                                *pinfo = *info;
                        }
                        crate::globals::log(&format!("GetInfo style=0x{:x}", info.dwStyle));
                        Ok(())
                })
        }

        fn GetStatus(&self) -> Result<u32> {
                guarded(|| {
                        let s = self.status.load(Ordering::Relaxed);
                        crate::globals::log(&format!("GetStatus -> 0x{s:x}"));
                        Ok(s)
                })
        }

        fn Show(&self, fshow: BOOL) -> Result<()> {
                crate::globals::log(&format!("langbar Show fshow={}", fshow.as_bool()));
                guarded(|| {
                        self.notify_update(TF_LBI_STATUS);
                        Ok(())
                })
        }

        fn GetTooltipString(&self) -> Result<BSTR> {
                guarded(|| {
                        crate::globals::log("GetTooltipString");
                        Ok(BSTR::from(self.tooltip.as_str()))
                })
        }
}

impl ITfLangBarItemButton_Impl for LangBarItem_Impl {
        fn OnClick(&self, click: TfLBIClick, pt: &POINT, _prcarea: *const RECT) -> Result<()> {
                crate::globals::log(&format!("langbar OnClick click={}", click.0));
                guarded(|| {
                        // TfLBIClick: RIGHT=1, LEFT=2 (yes, reversed vs the
                        // names — anything else isn't a real button press).
                        const TF_LBI_CLK_RIGHT: i32 = 1;
                        if click.0 == TF_LBI_CLK_RIGHT {
                                // TF_LBI_CLK_RIGHT — in tray rendering the
                                // shell calls OnClick and expects us to pop
                                // the menu ourselves (weasel does the same
                                // via TrackPopupMenuEx); InitMenu is only
                                // used by the classic desktop language bar.
                                //
                                // Defer via the refresh window: the modal
                                // popup must run after this click's
                                // button-up is consumed, else the menu sees
                                // the button-up as a click-away and closes
                                // instantly.
                                let item_ptr = std::ptr::from_ref(&**self) as usize;
                                crate::tray::post_settings_menu(item_ptr, pt.x, pt.y);
                                return Ok(());
                        }
                        let Some(compartment) = self.compartment() else {
                                return Err(Error::from_hresult(E_FAIL));
                        };
                        if let Ok(is_on) = compartment.get_bool() {
                                let _ = compartment.set_bool(!is_on);
                        }
                        Ok(())
                })
        }

        fn InitMenu(&self, pmenu: Ref<'_, ITfMenu>) -> Result<()> {
                crate::globals::log("langbar InitMenu");
                guarded(|| {
                        let Some(menu) = pmenu.as_ref() else { return Err(Error::from_hresult(E_INVALIDARG)) };
                        let handler = self.settings_handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        let Some(weak) = handler else { return Ok(()) };
                        let Some(processor) = weak.upgrade() else { return Ok(()) };
                        // try_lock — a blocking wait here hangs the shell's UI
                        // thread (explorer) when another thread holds the lock.
                        let Ok(processor) = processor.try_lock() else {
                                crate::globals::log("InitMenu: try_lock failed");
                                return Ok(());
                        };
                        let snapshot = build_menu_snapshot(&processor);
                        drop(processor);
                        crate::globals::log(&format!("InitMenu: adding {} items", snapshot.len()));
                        add_tf_items(menu, &snapshot)
                })
        }

        fn OnMenuSelect(&self, wid: u32) -> Result<()> {
                crate::globals::log(&format!("langbar OnMenuSelect id={wid}"));
                guarded(|| {
                        self.on_menu_select(wid);
                        Ok(())
                })
        }

        fn GetIcon(&self) -> Result<HICON> {
                guarded(|| self.get_icon_inner())
        }

        fn GetText(&self) -> Result<BSTR> {
                guarded(|| {
                        let info = self.info.lock().unwrap_or_else(|e| e.into_inner());
                        let len = info.szDescription.iter().position(|&c| c == 0).unwrap_or(32);
                        let text = String::from_utf16_lossy(&info.szDescription[..len]);
                        Ok(BSTR::from(text.as_str()))
                })
        }
}

impl LangBarItem_Impl {
        fn get_icon_inner(&self) -> Result<HICON> {
                use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_DEFAULTCOLOR};
                // Default to the Cantonese-mode icon when the compartment is
                // not yet registered. Caps Lock is a hard English override —
                // it doesn't touch the compartment, so it must be checked
                // here explicitly.
                let is_on = self
                        .compartment()
                        .and_then(|c| c.get_bool().ok())
                        .unwrap_or(true)
                        && !crate::keys::caps_lock_on();
                let status = self.status.load(Ordering::Relaxed);
                const TF_LBI_STATUS_DISABLED: u32 = 0x04;
                let cx = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXICON) };
                let cy = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYICON) };
                let index = if is_on && status & TF_LBI_STATUS_DISABLED == 0 {
                        self.on_icon
                } else {
                        self.off_icon
                };
                let handle = globals::dll_instance();
                if handle.0.is_null() {
                        return Err(Error::from_hresult(E_FAIL));
                }
                let icon = unsafe {
                        LoadImageW(
                                Some(handle),
                                windows::core::PCWSTR(index as usize as *const u16),
                                IMAGE_ICON,
                                cx,
                                cy,
                                LR_DEFAULTCOLOR,
                        )
                };
                match icon {
                        Ok(icon) if !icon.is_invalid() => {
                                crate::globals::log(&format!("GetIcon ok: is_on={is_on} index={index}"));
                                Ok(HICON(icon.0))
                        }
                        _ => {
                                crate::globals::log_error(&format!("GetIcon failed: is_on={is_on} index={index}"));
                                Err(Error::from_hresult(E_FAIL))
                        }
                }
        }
}

impl ITfSource_Impl for LangBarItem_Impl {
        fn AdviseSink(&self, riid: *const GUID, punk: Ref<'_, IUnknown>) -> Result<u32> {
                guarded(|| {
                        unsafe {
                                if *riid != ITfLangBarItemSink::IID {
                                        return Err(Error::from_hresult(CONNECT_E_CANNOTCONNECT));
                                }
                        }
                        if self.sink.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                                return Err(Error::from_hresult(CONNECT_E_ADVISELIMIT));
                        }
                        let Some(unknown) = punk.as_ref() else {
                                return Err(Error::from_hresult(E_INVALIDARG));
                        };
                        let sink: ITfLangBarItemSink = unknown.cast().map_err(|_| Error::from_hresult(E_NOINTERFACE))?;
                        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
                        crate::globals::log("langbar shell AdviseSink ok");
                        Ok(self.sink_cookie)
                })
        }

        fn UnadviseSink(&self, dwcookie: u32) -> Result<()> {
                crate::globals::log("langbar shell UnadviseSink");
                guarded(|| {
                        if dwcookie != self.sink_cookie {
                                return Err(Error::from_hresult(CONNECT_E_NOCONNECTION));
                        }
                        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
                        if sink.is_none() {
                                return Err(Error::from_hresult(CONNECT_E_NOCONNECTION));
                        }
                        *sink = None;
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// Popup settings menu (right click) — plain Win32 popup, replaces the
// custom SettingsMenuWindow from upstream for now.
// ---------------------------------------------------------------------

fn build_popup(menu: windows::Win32::UI::WindowsAndMessaging::HMENU, items: &[MenuItem]) {
        for item in items {
                match item {
                        MenuItem::Separator => unsafe {
                                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
                        },
                        MenuItem::Command { id, text, enabled, checked } => unsafe {
                                let mut flags = MF_STRING;
                                if !enabled {
                                        flags |= MF_GRAYED;
                                }
                                if *checked {
                                        flags |= MF_CHECKED;
                                }
                                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                                let _ = AppendMenuW(menu, flags, *id as usize, windows::core::PCWSTR(wide.as_ptr()));
                        },
                        MenuItem::Submenu { text, children, .. } => unsafe {
                                let sub = CreatePopupMenu().unwrap_or_default();
                                build_popup(sub, children);
                                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                                let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, windows::core::PCWSTR(wide.as_ptr()));
                        },
                }
        }
}

/// Modal popup run on a worker thread spawned by OnClick — keeps the shell's
/// call into the text service from blocking on a menu loop.
/// Show the settings popup at `pt` — used by the tray icon's host window.
/// Runs on the caller's thread (our own UI/worker thread, never the shell's).
pub fn show_settings_menu_at(pt: POINT, processor: &std::sync::Arc<Mutex<Processor>>) {
        crate::globals::log(&format!("show_settings_menu_at pt=({},{})", pt.x, pt.y));
        let snapshot = match processor.try_lock() {
                Ok(p) => build_menu_snapshot(&p),
                Err(_) => {
                        crate::globals::log("show_settings_menu_at: try_lock failed");
                        return;
                }
        };
        crate::globals::log(&format!("show_settings_menu_at: {} items", snapshot.len()));
        show_settings_menu_owned(pt, snapshot, std::sync::Arc::downgrade(processor));
}

fn show_settings_menu_owned(pt: POINT, items: Vec<MenuItem>, handler: Weak<Mutex<Processor>>) {
        unsafe {
                let menu = match CreatePopupMenu() {
                        Ok(m) => m,
                        Err(_) => {
                                crate::globals::log_error("show_settings_menu: CreatePopupMenu failed");
                                return;
                        }
                };
                build_popup(menu, &items);
                let mut point = pt;
                if point.x == 0 && point.y == 0 {
                        let _ = GetCursorPos(&mut point);
                }
                // Dedicated WS_POPUP owner created on THIS thread (same
                // pattern weasel's fix uses): a message-only or foreign
                // window can't own a modal menu. WS_EX_TOOLWINDOW keeps it
                // out of the taskbar/alt-tab.
                use windows::Win32::UI::WindowsAndMessaging::{
                        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WNDCLASSEXW, WS_POPUP, WS_EX_TOOLWINDOW,
                };
                extern "system" fn popup_owner_proc(
                        hwnd: HWND,
                        msg: u32,
                        w: WPARAM,
                        l: LPARAM,
                ) -> LRESULT {
                        unsafe { DefWindowProcW(hwnd, msg, w, l) }
                }
                let cls_name: Vec<u16> = "RCantonesePopupOwner".encode_utf16().chain(Some(0)).collect();
                let wc = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        lpszClassName: PCWSTR(cls_name.as_ptr()),
                        lpfnWndProc: Some(popup_owner_proc),
                        hInstance: globals::dll_instance().into(),
                        ..Default::default()
                };
                let _ = RegisterClassExW(&wc); // ok if already registered
                let owner = match CreateWindowExW(
                        WS_EX_TOOLWINDOW,
                        PCWSTR(cls_name.as_ptr()),
                        PCWSTR::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(globals::dll_instance().into()),
                        None,
                ) {
                        Ok(h) => h,
                        Err(e) => {
                                crate::globals::log_error(&format!("show_settings_menu: owner create failed {e:?}"));
                                let _ = DestroyMenu(menu);
                                return;
                        }
                };
                crate::globals::log(&format!("show_settings_menu: popup at ({},{}) hwnd={:?}", point.x, point.y, owner));
                // Q135788: the menu only dismisses on click-away if the
                // owner holds foreground — without this it stays open.
                let _ = SetForegroundWindow(owner);
                let cmd = TrackPopupMenuEx(menu, (TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD).0, point.x, point.y, owner, None);
                let gle = GetLastError();
                crate::globals::log(&format!("show_settings_menu: TrackPopupMenuEx -> {} gle=0x{:x}", cmd.0, gle.0));
                // Q135788: WM_NULL releases the menu's modal state cleanly.
                let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
                let _ = DestroyMenu(menu);
                let _ = DestroyWindow(owner);
                if cmd.0 != 0 {
                        if let Some(processor) = handler.upgrade() {
                                menu_command(&mut processor.lock().unwrap_or_else(|e| e.into_inner()), cmd.0 as u32);
                        }
                }
        }
}
