// COM / TSF registration — port of Register.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;

use crate::globals::{self, CLSID_RCANTONESE, GUID_PROFILE};

const TEXTSERVICE_LANGID: u16 = globals::TEXTSERVICE_LANGID;
const TEXTSERVICE_MODEL: &str = "Apartment";
const TEXTSERVICE_DESCRIPTION: &str = "R-Cantonese";

const SUPPORT_CATEGORIES: [GUID; 8] = [
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
        GUID_TFCAT_TIPCAP_SECUREMODE,
        GUID_TFCAT_TIPCAP_COMLESS,
        GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
];

// -- IMM32 (.ime) keyboard layout -------------------------------------------
// Weasel's dual-mode pattern: an E-series KLID under HKLM Keyboard Layouts
// pointing at our .ime file, then RegisterProfile links the TSF profile to
// it via hklSubstitute so IMM32 apps (WoW etc.) load the .ime directly.

const IME_FILE_NAME: &str = "r-cantonese.ime";
const IME_LAYOUT_TEXT: &str = "R-Cantonese";

fn e_series_klid(index: u32) -> String {
        format!("E0{:02X}{:04X}", index, TEXTSERVICE_LANGID)
}

/// Scan HKLM Keyboard Layouts for our IME's E-series KLID — weasel's
/// FindIME. Returns the HKL (KLID as u32) or None.
fn find_ime_hkl() -> Option<HKL> {
        unsafe {
                let base = wide_string("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts");
                let mut layouts = HKEY::default();
                if RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(base.as_ptr()), Some(0), KEY_READ, &mut layouts)
                        != ERROR_SUCCESS
                {
                        return None;
                }
                let mut found = None;
                for index in 0u32..=0xFF {
                        let klid = e_series_klid(index);
                        let sub = wide_string(&klid);
                        let mut key = HKEY::default();
                        if RegOpenKeyExW(layouts, PCWSTR(sub.as_ptr()), Some(0), KEY_READ, &mut key)
                                != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let mut value = [0u16; 64];
                        let mut size = (value.len() * 2) as u32;
                        let mut kind = REG_VALUE_TYPE(0);
                        let ok = RegQueryValueExW(
                                key,
                                w!("Ime File"),
                                None,
                                Some(&mut kind),
                                Some(value.as_mut_ptr() as *mut u8),
                                Some(&mut size),
                        );
                        let _ = RegCloseKey(key);
                        if ok == ERROR_SUCCESS {
                                let name = String::from_utf16_lossy(&value[..size as usize / 2])
                                        .trim_end_matches('\0')
                                        .to_string();
                                if name.eq_ignore_ascii_case(IME_FILE_NAME) {
                                        found = Some(HKL((u32::from_str_radix(&klid, 16).unwrap_or(0)) as *mut _));
                                        break;
                                }
                        }
                }
                let _ = RegCloseKey(layouts);
                found
        }
}

/// Install our .ime as an IMM32 keyboard layout. The file must already sit
/// in the system directory (installer copies it before regsvr32). Returns
/// the registered HKL. Idempotent — if a KLID already points at our file
/// we just reuse it.
fn install_imm32_ime() -> Option<HKL> {
        let hkl = install_imm32_ime_inner();
        if let Some(hkl) = hkl {
                // Fixups are cheap and idempotent — run them even when the
                // KLID already exists, because older installs may have
                // baked a plain-keyboard substitute into the CTF
                // assembly binding and user substitutes.
                let klid = format!("{:08X}", hkl.0 as usize as u32);
                // Legacy Substitutes map: activating the base zh-HK
                // keyboard yields our IME instead — the classic way
                // IMM32 apps (WoW/EVE) load .ime files.
                set_base_layout_substitute(&klid);
                // The per-user CTF assembly binding caches the keyboard
                // layout handed to legacy apps — if it was written before
                // the E-KLID existed it points at a plain keyboard and the
                // .ime never loads.
                fix_assembly_keyboard_layout(hkl);
        }
        hkl
}

fn install_imm32_ime_inner() -> Option<HKL> {
        if let Some(hkl) = find_ime_hkl() {
                return Some(hkl);
        }
        unsafe {
                let mut sysdir = [0u16; MAX_PATH as usize];
                let n = GetSystemDirectoryW(Some(&mut sysdir));
                if n == 0 || n as usize >= sysdir.len() {
                        return None;
                }
                let path = format!("{}\\{}", String::from_utf16_lossy(&sysdir[..n as usize]), IME_FILE_NAME);
                if !std::path::Path::new(&path).exists() {
                        return None;
                }
                let file = wide_string(&path);
                let text = wide_string(IME_LAYOUT_TEXT);
                let hkl = windows::Win32::UI::Input::Ime::ImmInstallIMEW(
                        PCWSTR(file.as_ptr()),
                        PCWSTR(text.as_ptr()),
                );
                if !hkl.0.is_null() {
                        // Real install — the layout is a genuine IME; no
                        // Substitutes hack needed.
                        return Some(hkl);
                }
                // ImmInstallIMEW returns NULL on failure — HKL::is_invalid
                // tests for INVALID_HANDLE_VALUE (-1), so check the raw
                // pointer here or we'd treat failure as success.
                globals::log_error("IME01: ImmInstallIMEW failed, manual KLID fallback");
                // Manual fallback — ImmInstallIMEW can decline when the E-
                // series space it wants is taken; scan for a free slot and
                // write the values directly (weasel's fallback). E020..E0FF
                // is the user-IME range — E000..E01F is reserved for system
                // IMEs (immdev/imm.h MIN_USER_IMM_IME_ID).
                for index in 0x20u32..=0xFF {
                        let klid = e_series_klid(index);
                        let sub = format!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid}");
                        let sub_wide = wide_string(&sub);
                        let mut key = HKEY::default();
                        let status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(sub_wide.as_ptr()), Some(0), KEY_READ, &mut key);
                        if status == ERROR_SUCCESS {
                                let _ = RegCloseKey(key);
                                continue; // occupied
                        }
                        let mut created = HKEY::default();
                        if RegCreateKeyExW(
                                HKEY_LOCAL_MACHINE,
                                PCWSTR(sub_wide.as_ptr()),
                                Some(0),
                                PCWSTR::null(),
                                REG_OPTION_NON_VOLATILE,
                                KEY_WRITE,
                                None,
                                &mut created,
                                None,
                        ) != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let set = |name: &[u16], value: &[u16]| {
                                let bytes = unsafe {
                                        std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2)
                                };
                                let _ = RegSetValueExW(created, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(bytes));
                        };
                        set(&wide_string("Ime File"), &wide_string(IME_FILE_NAME));
                        set(&wide_string("Layout File"), &wide_string("kbdus.dll"));
                        set(&wide_string("Layout Text"), &wide_string(IME_LAYOUT_TEXT));
                        let _ = RegCloseKey(created);
                        // Weasel's manual path also adds the HKL to the
                        // user's Preload list — without it the layout
                        // can't be activated by CTF substitution or
                        // LoadKeyboardLayout at all.
                        add_ime_to_preload(&klid);
                        // Legacy Substitutes map: activating the base zh-HK
                        // keyboard yields our IME instead — the classic way
                        // IMM32 apps (WoW/EVE) load .ime files.
                        set_base_layout_substitute(&klid);
                        return Some(HKL(u32::from_str_radix(&klid, 16).unwrap_or(0) as *mut _));
                }
                None
        }
}

/// Append an IME KLID to HKCU\Keyboard Layout\Preload — weasel's
/// registration does this whenever the KLID was written manually; the
/// preload entry makes the layout actually loadable on threads.
fn add_ime_to_preload(klid: &str) {
        let path = wide_string("Keyboard Layout\\Preload");
        let mut key = HKEY::default();
        unsafe {
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), Some(0), KEY_READ | KEY_WRITE, &mut key)
                        != ERROR_SUCCESS
                {
                        globals::log_error("IME01: Preload key open failed");
                        return;
                }
                for i in 1u32..=20 {
                        let name = wide_string(&i.to_string());
                        let mut buf = [0u16; 16];
                        let mut size = (buf.len() * 2) as u32;
                        let exists = RegQueryValueExW(
                                key,
                                PCWSTR(name.as_ptr()),
                                None,
                                None,
                                Some(buf.as_mut_ptr() as *mut u8),
                                Some(&mut size),
                        ) == ERROR_SUCCESS;
                        if !exists {
                                let value = wide_string(&klid.to_uppercase());
                                let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
                                let ok = RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(bytes));
                                globals::log_error(&format!("IME01: Preload[{i}]={klid} set={ok:?}"));
                                break;
                        }
                }
                let _ = RegCloseKey(key);
        }
}

/// Remove our E-series KLID + its preload entry on uninstall.
fn uninstall_imm32_ime() {
        let Some(hkl) = find_ime_hkl() else { return };
        let klid = format!("{:08X}", hkl.0 as usize as u32);
        let sub = format!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid}");
        unsafe {
                let _ = recurse_delete_key(HKEY_LOCAL_MACHINE, &wide_string(&sub));
        }
        remove_ime_from_preload(&klid);
        remove_base_layout_substitute();
}

/// Map the langid's base keyboard layout (e.g. 00000C04 for zh-HK) to
/// our E-KLID via HKCU\Keyboard Layout\Substitutes — legacy activation
/// path for IMM32 apps. Proven on 09-28: without this the .ime never
/// loads even though the KLID + Preload entries exist.
fn set_base_layout_substitute(klid: &str) {
        let base = format!("{:08x}", TEXTSERVICE_LANGID);
        let path = wide_string("Keyboard Layout\\Substitutes");
        let mut key = HKEY::default();
        unsafe {
                if RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        PCWSTR(path.as_ptr()),
                        Some(0),
                        PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_WRITE,
                        None,
                        &mut key,
                        None,
                ) != ERROR_SUCCESS
                {
                        globals::log_error("IME01: Substitutes key create failed");
                        return;
                }
                let name = wide_string(&base);
                let value = wide_string(&klid.to_uppercase());
                let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
                let ok = RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(bytes));
                globals::log_error(&format!("IME01: Substitutes[{base}]={klid} set={ok:?}"));
                let _ = RegCloseKey(key);
        }
}

/// Rewrite the per-user CTF assembly binding's KeyboardLayout to our
/// E-KLID. `HKCU\Software\Microsoft\CTF\Assemblies\<langid>\<assembly>`
/// caches the layout legacy apps get when our TIP is selected — entries
/// created before the .ime existed pin a plain keyboard (e.g. 0x04090C04)
/// so the IME never loads in IMM32 apps. Only touches assemblies whose
/// Default is our CLSID.
fn fix_assembly_keyboard_layout(ime_hkl: HKL) {
        let root_path = wide_string("SOFTWARE\\Microsoft\\CTF\\Assemblies");
        let our_clsid = format!("{:?}", CLSID_RCANTONESE).to_uppercase();
        let mut root = HKEY::default();
        unsafe {
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(root_path.as_ptr()), Some(0), KEY_READ, &mut root)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut lang = [0u16; 64];
                let mut i = 0u32;
                loop {
                        let mut len = lang.len() as u32;
                        if RegEnumKeyExW(root, i, Some(PWSTR(lang.as_mut_ptr())), &mut len, None, None, None, None)
                                != ERROR_SUCCESS
                        {
                                break;
                        }
                        i += 1;
                        let lang_name = String::from_utf16_lossy(&lang[..len as usize]);
                        for assembly in ["{34745C63-B2F0-4784-8B67-5E12C8701A31}"] {
                                let sub = format!("SOFTWARE\\Microsoft\\CTF\\Assemblies\\{lang_name}\\{assembly}");
                                let sub_w = wide_string(&sub);
                                let mut key = HKEY::default();
                                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub_w.as_ptr()), Some(0), KEY_READ | KEY_WRITE, &mut key)
                                        != ERROR_SUCCESS
                                {
                                        continue;
                                }
                                let mut buf = [0u16; 64];
                                let mut size = (buf.len() * 2) as u32;
                                let ok = RegQueryValueExW(
                                        key,
                                        w!("Default"),
                                        None,
                                        None,
                                        Some(buf.as_mut_ptr() as *mut u8),
                                        Some(&mut size),
                                ) == ERROR_SUCCESS;
                                if ok {
                                        let cur = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]);
                                        if cur.trim_matches(char::from(0)).eq_ignore_ascii_case(&our_clsid) {
                                                let _ = RegSetValueExW(
                                                        key,
                                                        w!("KeyboardLayout"),
                                                        Some(0),
                                                        REG_DWORD,
                                                        Some(&(ime_hkl.0 as usize as u32).to_le_bytes()),
                                                );
                                                globals::log(&format!(
                                                        "IME01: assembly {lang_name} KeyboardLayout -> {:08X}",
                                                        ime_hkl.0 as usize as u32
                                                ));
                                        }
                                }
                                let _ = RegCloseKey(key);
                        }
                }
                let _ = RegCloseKey(root);
        }
}

/// Drop our substitution on the base layout (restore whatever was there
/// is not possible — just delete; US-keyboard substitutes regenerate).
fn remove_base_layout_substitute() {
        let base = format!("{:08x}", TEXTSERVICE_LANGID);
        let path = wide_string("Keyboard Layout\\Substitutes");
        let mut key = HKEY::default();
        unsafe {
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), Some(0), KEY_READ | KEY_WRITE, &mut key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut buf = [0u16; 16];
                let mut size = (buf.len() * 2) as u32;
                let name = wide_string(&base);
                if RegQueryValueExW(
                        key,
                        PCWSTR(name.as_ptr()),
                        None,
                        None,
                        Some(buf.as_mut_ptr() as *mut u8),
                        Some(&mut size),
                ) == ERROR_SUCCESS
                {
                        let val = String::from_utf16_lossy(&buf[..size as usize / 2])
                                .trim_end_matches('\0')
                                .to_string();
                        // Only remove if it still points at our KLID.
                        if val.to_uppercase().starts_with('E') {
                                let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
                        }
                }
                let _ = RegCloseKey(key);
        }
}

/// Drop our KLID from HKCU\Keyboard Layout\Preload.
fn remove_ime_from_preload(klid: &str) {
        let path = wide_string("Keyboard Layout\\Preload");
        let mut key = HKEY::default();
        unsafe {
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), Some(0), KEY_READ | KEY_WRITE, &mut key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                for i in 1u32..=20 {
                        let name = wide_string(&i.to_string());
                        let mut buf = [0u16; 16];
                        let mut size = (buf.len() * 2) as u32;
                        if RegQueryValueExW(
                                key,
                                PCWSTR(name.as_ptr()),
                                None,
                                None,
                                Some(buf.as_mut_ptr() as *mut u8),
                                Some(&mut size),
                        ) != ERROR_SUCCESS
                        {
                                break;
                        }
                        let val = String::from_utf16_lossy(&buf[..size as usize / 2])
                                .trim_end_matches('\0')
                                .to_string();
                        if val.eq_ignore_ascii_case(klid) {
                                let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
                                break;
                        }
                }
                let _ = RegCloseKey(key);
        }
}

fn is_repeat_registration_success(result: &Result<()>) -> bool {
        const TF_E_ALREADY_EXISTS_HR: HRESULT = HRESULT(0x80041F0Au32 as i32);
        match result {
                Ok(()) => true,
                Err(e) => e.code() == TF_E_ALREADY_EXISTS_HR || e.code() == HRESULT::from_win32(ERROR_ALREADY_EXISTS.0),
        }
}

fn guid_to_string(guid: &GUID) -> String {
        let mut buffer = [0u16; 64];
        unsafe {
                let len = StringFromGUID2(guid, &mut buffer);
                if len <= 0 {
                        return "<invalid-guid>".to_string();
                }
                String::from_utf16_lossy(&buffer[..(len as usize - 1).min(buffer.len())])
        }
}

fn clsid_key_path() -> String {
        format!("CLSID\\{}", guid_to_string(&CLSID_RCANTONESE))
}

/// Per-user variant for non-elevated registration: HKCU\Software\Classes\CLSID\{...}
fn clsid_key_path_user() -> String {
        format!("Software\\Classes\\CLSID\\{}", guid_to_string(&CLSID_RCANTONESE))
}

fn wide_string(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn module_file_name() -> Option<Vec<u16>> {
        let mut buffer = vec![0u16; 1024];
        unsafe {
                let len = GetModuleFileNameW(Some(globals::dll_instance().into()), &mut buffer);
                if len == 0 || len as usize >= buffer.len() {
                        return None;
                }
                buffer.truncate(len as usize + 1);
                Some(buffer)
        }
}

fn textservice_description() -> String {
        crate::strings::text_or(crate::strings::IDS_TEXTSERVICE_DESC, TEXTSERVICE_DESCRIPTION).to_string()
}

pub fn register_profiles() -> bool {
        unsafe {
                let profile_mgr: ITfInputProcessorProfileMgr = match CoCreateInstance(
                        &CLSID_TF_InputProcessorProfiles,
                        None,
                        CLSCTX_INPROC_SERVER,
                ) {
                        Ok(mgr) => mgr,
                        Err(_) => {
                                globals::log_error("RegisterProfiles: CoCreateInstance failed");
                                return false;
                        }
                };

                let Some(icon_file) = module_file_name() else {
                        globals::log_error("RegisterProfiles: GetModuleFileName failed");
                        return false;
                };
                let icon_path: Vec<u16> = icon_file[..icon_file.len() - 1].to_vec();

                let description = textservice_description();
                let desc_wide = wide_string(&description);

                // Upstream uses -IDIS_IME — negative icon index = resource id (ExtractIcon convention).
                let icon_index = (0i32 - globals::TEXTSERVICE_ICON_INDEX as i32) as u32;

                // Link the profile to our IMM32 .ime via an E-series
                // keyboard layout — IMM32 apps (WoW etc.) load the .ime
                // directly instead of going through the CTF bridge.
                let substitute = install_imm32_ime().unwrap_or_default();
                if !substitute.is_invalid() {
                        // Re-register so upgrades pick up the substitute —
                        // RegisterProfile alone early-exits on existing
                        // profiles and would never update hklSubstitute.
                        let _ = profile_mgr.UnregisterProfile(&CLSID_RCANTONESE, TEXTSERVICE_LANGID, &GUID_PROFILE, 0);
                }
                globals::log(&format!("RegisterProfiles: hklSubstitute = {:?}", substitute));

                let result = profile_mgr.RegisterProfile(
                        &CLSID_RCANTONESE,
                        TEXTSERVICE_LANGID,
                        &GUID_PROFILE,
                        &desc_wide[..desc_wide.len() - 1],
                        &icon_path,
                        icon_index,
                        substitute,
                        0,
                        true,
                        0,
                );

                if !is_repeat_registration_success(&result) {
                        let code = result.err().map(|e| e.code()).unwrap_or(HRESULT(0));
                        globals::log_error(&format!("RegisterProfiles: RegisterProfile failed: {code:?}"));
                        return false;
                }
                // InstallLayoutOrTip must run in the *user's* context — when
                // called elevated (regsvr32 RunAs admin) it lands on the
                // .DEFAULT hive instead of the user profile, so the tip
                // never enters the language list and stray default
                // keyboards get added there. The tray exe enables the tip
                // at startup instead.
                if !is_process_elevated() {
                        install_layout_or_tip(false);
                }
                true
        }
}

/// Whether the current process runs with an elevated token.
fn is_process_elevated() -> bool {
        use windows::Win32::Security::*;
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
                let mut token = HANDLE::default();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                        return false;
                }
                let mut elevation = TOKEN_ELEVATION::default();
                let mut len = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
                let ok = GetTokenInformation(
                        token,
                        TokenElevation,
                        Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
                        len,
                        &mut len,
                )
                .is_ok();
                let _ = CloseHandle(token);
                ok && elevation.TokenIsElevated != 0
        }
}

// Static raw-dylib import via windows-link — replaces the LoadLibrary +
// GetProcAddress + transmute dance. input.dll is present on every
// supported Windows; the import only resolves when our dll loads, which
// happens under regsvr32/tray anyway.
windows_link::link!("input.dll" "system" fn InstallLayoutOrTip(psz: PCWSTR, flags: u32) -> BOOL);

/// Enable/disable the profile in the user's input list via
/// input.dll!InstallLayoutOrTip — weasel's mechanism; surgical, never
/// rewrites the whole language list (that drops other IMEs' tips).
pub fn install_layout_or_tip(uninstall: bool) {
        const ILOT_UNINSTALL: u32 = 0x1;
        unsafe {
                let title = format!(
                        "{:04X}:{}{}",
                        TEXTSERVICE_LANGID,
                        guid_to_string(&CLSID_RCANTONESE),
                        guid_to_string(&GUID_PROFILE)
                );
                let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
                let flags = if uninstall { ILOT_UNINSTALL } else { 0 };
                let ok = InstallLayoutOrTip(PCWSTR(wide.as_ptr()), flags);
                if !ok.as_bool() {
                        globals::log_error(&format!("InstallLayoutOrTip(uninstall={uninstall}) failed"));
                }
        }
}

pub fn unregister_profiles() {
        unsafe {
                uninstall_imm32_ime();
                install_layout_or_tip(true);
                let profile_mgr: ITfInputProcessorProfileMgr = match CoCreateInstance(
                        &CLSID_TF_InputProcessorProfiles,
                        None,
                        CLSCTX_INPROC_SERVER,
                ) {
                        Ok(mgr) => mgr,
                        Err(_) => return,
                };
                let _ = profile_mgr.UnregisterProfile(&CLSID_RCANTONESE, TEXTSERVICE_LANGID, &GUID_PROFILE, 0);
        }
}

pub fn register_categories() -> bool {
        unsafe {
                let category_mgr: ITfCategoryMgr = match CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
                        Ok(mgr) => mgr,
                        Err(_) => {
                                globals::log_error("RegisterCategories: CoCreateInstance failed");
                                return false;
                        }
                };
                for guid in &SUPPORT_CATEGORIES {
                        let result = category_mgr.RegisterCategory(&CLSID_RCANTONESE, guid, &CLSID_RCANTONESE);
                        if !is_repeat_registration_success(&result) {
                                let code = result.err().map(|e| e.code()).unwrap_or(HRESULT(0));
                                globals::log_error(&format!("RegisterCategories: RegisterCategory failed: {code:?}"));
                                return false;
                        }
                }
                true
        }
}

pub fn unregister_categories() {
        unsafe {
                let category_mgr: ITfCategoryMgr = match CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
                        Ok(mgr) => mgr,
                        Err(_) => return,
                };
                for guid in &SUPPORT_CATEGORIES {
                        let _ = category_mgr.UnregisterCategory(&CLSID_RCANTONESE, guid, &CLSID_RCANTONESE);
                }
        }
}

fn recurse_delete_key(parent: HKEY, key: &[u16]) -> u32 {
        unsafe {
                let mut handle = HKEY::default();
                if RegOpenKeyExW(parent, PCWSTR(key.as_ptr()), Some(0), KEY_ALL_ACCESS, &mut handle) != ERROR_SUCCESS {
                        return ERROR_SUCCESS.0;
                }
                let mut status = ERROR_SUCCESS.0;
                let mut name_buffer = [0u16; 256];
                loop {
                        let mut size = name_buffer.len() as u32;
                        if RegEnumKeyExW(
                                handle,
                                0,
                                Some(PWSTR(name_buffer.as_mut_ptr())),
                                &mut size,
                                None,
                                None,
                                None,
                                None,
                        ) != ERROR_SUCCESS
                        {
                                break;
                        }
                        let name_len = size as usize;
                        status = recurse_delete_key(handle, &name_buffer[..name_len + 1]);
                        if status != ERROR_SUCCESS.0 {
                                break;
                        }
                }
                let _ = RegCloseKey(handle);
                if status == ERROR_SUCCESS.0 {
                        RegDeleteKeyW(parent, PCWSTR(key.as_ptr())).0
                } else {
                        status
                }
        }
}

pub fn register_server() -> bool {
        register_server_at(HKEY_CLASSES_ROOT, &clsid_key_path()) || register_server_at(HKEY_CURRENT_USER, &clsid_key_path_user())
}

fn register_server_at(root: HKEY, path: &str) -> bool {
        unsafe {
                let key_path = wide_string(path);
                let description = textservice_description();
                let desc_wide = wide_string(&description);

                let mut key = HKEY::default();
                if RegCreateKeyExW(
                        root,
                        PCWSTR(key_path.as_ptr()),
                        Some(0),
                        PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_WRITE,
                        None,
                        &mut key,
                        None,
                ) != ERROR_SUCCESS
                {
                        return false;
                }

                let mut ok = RegSetValueExW(
                        key,
                        PCWSTR::null(),
                        Some(0),
                        REG_SZ,
                        Some(std::slice::from_raw_parts(desc_wide.as_ptr() as *const u8, desc_wide.len() * 2)),
                ) == ERROR_SUCCESS;

                if ok {
                        let subkey_name = wide_string("InProcServer32");
                        let mut subkey = HKEY::default();
                        if RegCreateKeyExW(
                                key,
                                PCWSTR(subkey_name.as_ptr()),
                                Some(0),
                                PCWSTR::null(),
                                REG_OPTION_NON_VOLATILE,
                                KEY_WRITE,
                                None,
                                &mut subkey,
                                None,
                        ) == ERROR_SUCCESS
                        {
                                let Some(path) = module_file_name() else {
                                        let _ = RegCloseKey(subkey);
                                        let _ = RegCloseKey(key);
                                        return false;
                                };
                                ok = RegSetValueExW(
                                        subkey,
                                        PCWSTR::null(),
                                        Some(0),
                                        REG_SZ,
                                        Some(std::slice::from_raw_parts(path.as_ptr() as *const u8, path.len() * 2)),
                                ) == ERROR_SUCCESS;
                                if ok {
                                        let model = wide_string(TEXTSERVICE_MODEL);
                                        let model_name = wide_string("ThreadingModel");
                                        ok = RegSetValueExW(
                                                subkey,
                                                PCWSTR(model_name.as_ptr()),
                                                Some(0),
                                                REG_SZ,
                                                Some(std::slice::from_raw_parts(model.as_ptr() as *const u8, model.len() * 2)),
                                        ) == ERROR_SUCCESS;
                                }
                                let _ = RegCloseKey(subkey);
                        } else {
                                ok = false;
                        }
                }
                let _ = RegCloseKey(key);
                ok
        }
}

/// Weasel-style autostart: r-cantonese-tray.exe runs at user login so the
/// tray icon host exists before any process activates the IME. HKCU — no
/// elevation needed.
pub fn register_tray_autostart() {
        unsafe {
                let run = wide_string("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
                let mut key = HKEY::default();
                if RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        PCWSTR(run.as_ptr()),
                        Some(0),
                        PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_WRITE,
                        None,
                        &mut key,
                        None,
                ) != ERROR_SUCCESS
                {
                        return;
                }
                if let Some(dll) = module_file_name() {
                        let dll_dir = String::from_utf16_lossy(&dll[..dll.len() - 1]);
                        if let Some(dir) = dll_dir.rsplit_once('\\').map(|(d, _)| d.to_string()) {
                                let exe = format!("\"{}\\r-cantonese-tray.exe\"", dir);
                                let val = wide_string(&exe);
                                let name = wide_string("RCantoneseTray");
                                let _ = RegSetValueExW(
                                        key,
                                        PCWSTR(name.as_ptr()),
                                        Some(0),
                                        REG_SZ,
                                        Some(std::slice::from_raw_parts(val.as_ptr() as *const u8, val.len() * 2)),
                                );
                        }
                }
                let _ = RegCloseKey(key);
        }
}

pub fn unregister_tray_autostart() {
        unsafe {
                let run = wide_string("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
                let mut key = HKEY::default();
                if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(run.as_ptr()), Some(0), KEY_WRITE, &mut key) == ERROR_SUCCESS {
                        let name = wide_string("RCantoneseTray");
                        let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
                        let _ = RegCloseKey(key);
                }
        }
}

pub fn unregister_server() {
        let key_path = wide_string(&clsid_key_path());
        let status = recurse_delete_key(HKEY_CLASSES_ROOT, &key_path);
        if status != ERROR_SUCCESS.0 {
                globals::log_error("UnregisterServer: RecurseDeleteKey failed");
        }
        let user_path = wide_string(&clsid_key_path_user());
        let status = recurse_delete_key(HKEY_CURRENT_USER, &user_path);
        if status != ERROR_SUCCESS.0 {
                globals::log_error("UnregisterServer: user RecurseDeleteKey failed");
        }
}
