// COM / TSF registration — port of Register.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryW};
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

                let result = profile_mgr.RegisterProfile(
                        &CLSID_RCANTONESE,
                        TEXTSERVICE_LANGID,
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

/// Enable/disable the profile in the user's input list via
/// input.dll!InstallLayoutOrTip — weasel's mechanism; surgical, never
/// rewrites the whole language list (that drops other IMEs' tips).
pub fn install_layout_or_tip(uninstall: bool) {
        const ILOT_UNINSTALL: u32 = 0x1;
        unsafe {
                let module = match LoadLibraryW(w!("input.dll")) {
                        Ok(m) => m,
                        Err(e) => {
                                globals::log_error(&format!("InstallLayoutOrTip: input.dll load failed {e:?}"));
                                return;
                        }
                };
                let Some(proc_addr) = GetProcAddress(module, s!("InstallLayoutOrTip")) else {
                        globals::log_error("InstallLayoutOrTip: proc not found");
                        let _ = FreeLibrary(module);
                        return;
                };
                let f: unsafe extern "system" fn(PCWSTR, u32) -> BOOL = std::mem::transmute(proc_addr);
                let title = format!(
                        "{:04X}:{}{}",
                        TEXTSERVICE_LANGID,
                        guid_to_string(&CLSID_RCANTONESE),
                        guid_to_string(&GUID_PROFILE)
                );
                let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
                let flags = if uninstall { ILOT_UNINSTALL } else { 0 };
                let ok = f(PCWSTR(wide.as_ptr()), flags);
                if !ok.as_bool() {
                        globals::log_error(&format!("InstallLayoutOrTip(uninstall={uninstall}) failed"));
                }
                let _ = FreeLibrary(module);
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
