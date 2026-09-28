// COM / TSF registration — port of Register.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;

use crate::globals::{self, CLSID_RCANTONESE, GUID_PROFILE};

const TEXTSERVICE_LANGID: u16 = globals::TEXTSERVICE_LANGID;
const TEXTSERVICE_MODEL: &str = "Apartment";
const TEXTSERVICE_DESCRIPTION: &str = "R-Cantonese";

// {85F9794B-4D19-40D8-8864-4E747371A66D} — GUID_TFCAT_PROPSTYLE_CUSTOM
const GUID_TFCAT_PROPSTYLE_CUSTOM: GUID = GUID::from_u128(0x85f9794b_4d19_40d8_8864_4e747371a66d);
// {24AF3031-852D-40A2-BC09-8992898CE722} — GUID_TFCAT_PROPSTYLE_STATICCOMPACT
const GUID_TFCAT_PROPSTYLE_STATICCOMPACT: GUID = GUID::from_u128(0x24af3031_852d_40a2_bc09_8992898ce722);

/// Same set Weasel registers (WeaselTSF/Register.cpp) — notably TIPCAP_COMLESS
/// so TSF can host us in processes that never CoInitialize (games etc.).
const SUPPORT_CATEGORIES: [GUID; 16] = [
        GUID_TFCAT_CATEGORY_OF_TIP,
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_TIPCAP_SECUREMODE,
        GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
        GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
        GUID_TFCAT_TIPCAP_COMLESS,
        GUID_TFCAT_TIPCAP_WOW16,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        GUID_TFCAT_PROP_AUDIODATA,
        GUID_TFCAT_PROP_INKDATA,
        GUID_TFCAT_PROPSTYLE_CUSTOM,
        GUID_TFCAT_PROPSTYLE_STATIC,
        GUID_TFCAT_PROPSTYLE_STATICCOMPACT,
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_DISPLAYATTRIBUTEPROPERTY,
];

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
                // The x86 dll's registration overwrote IconFile with the
                // virtualized "System32\r-cantonese-x86.dll" — a name that
                // doesn't exist outside WOW64, so the profile icon fell back
                // to a generic tile. IconFile must name the real x64 image.
                let icon_text = String::from_utf16_lossy(&icon_file);
                let icon_text = icon_text.trim_end_matches('\0');
                let icon_file: Vec<u16> = if icon_text.to_lowercase().ends_with("\\r-cantonese-x86.dll") {
                        let mut dir = [0u16; 260];
                        let n = unsafe { GetWindowsDirectoryW(Some(&mut dir)) } as usize;
                        format!("{}\\System32\\r-cantonese.dll", String::from_utf16_lossy(&dir[..n]))
                                .encode_utf16()
                                .chain(Some(0))
                                .collect()
                } else {
                        icon_file
                };
                let icon_path: Vec<u16> = icon_file[..icon_file.len() - 1].to_vec();

                let description = textservice_description();
                let desc_wide = wide_string(&description);

                // Upstream uses -IDIS_IME — negative icon index = resource id (ExtractIcon convention).
                let icon_index = (0i32 - globals::TEXTSERVICE_ICON_INDEX as i32) as u32;

                // Pure TSF — no hklSubstitute. Register under both langids:
                // 0x0c04 is the canonical zh-HK profile; 0x0404 (zh-TW) is the
                // bridge path Weasel uses for HK users — cicero-unaware apps
                // get the 0x04040404 dummy, which hosts our TIP in-process.
                for langid in [TEXTSERVICE_LANGID, globals::TEXTSERVICE_BRIDGE_LANGID] {
                        let result = profile_mgr.RegisterProfile(
                                &CLSID_RCANTONESE,
                                langid,
                                &GUID_PROFILE,
                                &desc_wide[..desc_wide.len() - 1],
                                &icon_path,
                                icon_index,
                                HKL::default(),
                                0,
                                true,
                                0,
                        );

                        if !is_repeat_registration_success(&result) {
                                let code = result.err().map(|e| e.code()).unwrap_or(HRESULT(0));
                                globals::log_error(&format!("RegisterProfiles: RegisterProfile {langid:#06x} failed: {code:?}"));
                                return false;
                        }
                        // RegisterProfile keeps the existing IconFile on
                        // repeat registration (TF_E_ALREADY_EXISTS) — rewrite
                        // it directly so upgrades pick up the new path.
                        write_profile_icon(langid, &icon_file);
                        globals::log_error(&format!(
                                "RegisterProfiles: {langid:#06x} result ok, icon={}",
                                String::from_utf16_lossy(&icon_file[..icon_file.len().saturating_sub(1)])
                        ));
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

/// Rewrite IconFile/IconIndex on an existing profile — RegisterProfile skips
/// them when the profile already exists, so upgrades would keep pointing at
/// a stale dll path.
fn write_profile_icon(langid: u16, icon_file: &[u16]) {
        unsafe {
                let path = format!(
                        "SOFTWARE\\Microsoft\\CTF\\TIP\\{}\\LanguageProfile\\{:#010x}\\{}",
                        guid_to_string(&CLSID_RCANTONESE),
                        langid,
                        guid_to_string(&GUID_PROFILE)
                );
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                let mut key = HKEY::default();
                // TIP profiles live in the native 64-bit view — the x86
                // registration run must not let WOW64 redirect us into
                // WOW6432Node.
                let create = RegCreateKeyExW(
                        HKEY_LOCAL_MACHINE,
                        PCWSTR(wide.as_ptr()),
                        Some(0),
                        PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                        None,
                        &mut key,
                        None,
                );
                if create != ERROR_SUCCESS {
                        globals::log_error(&format!("write_profile_icon: RegCreateKeyExW failed {:?}", create));
                        return;
                }
                let name: Vec<u16> = "IconFile".encode_utf16().chain(Some(0)).collect();
                let bytes = std::slice::from_raw_parts(icon_file.as_ptr() as *const u8, icon_file.len() * 2);
                let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(bytes));
                let name: Vec<u16> = "IconIndex".encode_utf16().chain(Some(0)).collect();
                let index = (0i32 - globals::TEXTSERVICE_ICON_INDEX as i32) as u32;
                let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_DWORD, Some(&index.to_le_bytes()));
                let _ = RegCloseKey(key);
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
/// Uses the 0404 langid — the tips entry lands under zh-TW there, which is
/// also the bridge-friendly profile cicero-unaware apps resolve.
pub fn install_layout_or_tip(uninstall: bool) {
        const ILOT_UNINSTALL: u32 = 0x1;
        unsafe {
                let title = format!(
                        "{:04X}:{}{}",
                        globals::TEXTSERVICE_BRIDGE_LANGID,
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
                install_layout_or_tip(true);
                let profile_mgr: ITfInputProcessorProfileMgr = match CoCreateInstance(
                        &CLSID_TF_InputProcessorProfiles,
                        None,
                        CLSCTX_INPROC_SERVER,
                ) {
                        Ok(mgr) => mgr,
                        Err(_) => return,
                };
                for langid in [TEXTSERVICE_LANGID, globals::TEXTSERVICE_BRIDGE_LANGID] {
                        let _ = profile_mgr.UnregisterProfile(&CLSID_RCANTONESE, langid, &GUID_PROFILE, 0);
                }
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
