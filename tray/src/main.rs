// r-cantonese-tray.exe — user-session helper, WeaselServer-style.
//
// It does not own a notification-area icon. The 中/A button beside the
// input indicator is the langbar item. This process serves settings.toml
// on a named pipe, repairs the per-user input-tip registration, and keeps
// a message window so hosts can reload settings and clear learned words.

#![windows_subsystem = "windows"]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

mod settings_server;

const WM_TRAY_QUIT: u32 = WM_APP + 42;
const WM_TRAY_PING: u32 = WM_APP + 48; // liveness probe — returns TRAY_MAGIC
const TRAY_MAGIC: isize = 0x5243; // 'RC'

const TRAY_WND_CLASS: PCWSTR = w!("RCantoneseTrayIconWnd");
/// Our input tip — `{langid}:{clsid}{profile_guid}` for InstallLayoutOrTip.
/// The user-visible entry is the 0404 (zh-TW-langid) profile — Weasel's
/// approach for HK users: cicero-unaware apps then get the 0x04040404
/// dummy which hosts our TIP through the CTF bridge.
const INPUT_TIP: &str = "0404:{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";
const INPUT_TIP_0C04: &str = "0C04:{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";

// windows-link raw-dylib import — no import lib / GetProcAddress needed.
windows_link::link!("input.dll" "system" fn InstallLayoutOrTip(psz: PCWSTR, flags: u32) -> BOOL);

/// Enable the IME in the user's input list — InstallLayoutOrTip must run in
/// a non-elevated context (from regsvr32/admin it lands on .DEFAULT), and
/// the tray runs as the logged-in user at every login anyway.
fn ensure_input_tip() {
        unsafe {
                // The config center's locale switch decides which profile is
                // registered. Only the zh-HK install keeps the 0x0c04 profile —
                // gate the zh-Hant-HK tips entry on it so switching to zh-CN /
                // zh-TW doesn't leave a stray zh-HK entry we re-add each login.
                // zh-CN / zh-TW must be re-applied with InstallLayoutOrTip:
                // Settings and the default-input list only show a tip that is
                // attached to that language, and a previous login may have
                // swept it while this process still thought zh-HK was active.
                let cn_active = profile_enabled(0x0804);
                let hk_active = !cn_active && profile_registered(0x0c04);
                let tw_active = !cn_active && !hk_active && profile_registered(0x0404);
                if hk_active {
                        ensure_hk_tip_entry();
                        // If zh-HK itself isn't in the user's language list the tip has
                        // nowhere to attach (seen on zh-CN-only systems). Installing the
                        // zh-HK "US" keyboard forces the language into the list; our tip
                        // then sits beside it. Cost: a spare "US" keyboard under zh-HK
                        // in Settings — only on machines that lacked the language.
                        if !user_has_language_tag(&["zh-hant-hk", "zh-hk"]) {
                                let kb: Vec<u16> = "0C04:00000409".encode_utf16().chain(Some(0)).collect();
                                if !InstallLayoutOrTip(PCWSTR(kb.as_ptr()), 0).as_bool() {
                                        log_error("InstallLayoutOrTip failed (zh-HK language)");
                                }
                        }
                } else if cn_active {
                        ensure_lang_tip(0x0804, &["zh-hans-cn", "zh-cn"]);
                } else if tw_active {
                        ensure_lang_tip(0x0404, &["zh-hant-tw", "zh-tw"]);
                }
                // Drop our tips entries under languages that aren't the active
                // locale — config-center sweeps at switch time, this keeps it
                // consistent for machines that already accumulated them.
                let keep = if hk_active {
                        Some("zh-hant-hk")
                } else if cn_active {
                        Some("zh-hans-cn")
                } else if tw_active {
                        Some("zh-hant-tw")
                } else {
                        None
                };
                if let Some(keep) = keep {
                        sweep_tips_in(HKEY_CURRENT_USER, "Control Panel\\International\\User Profile", Some(keep));
                }
                clean_dead_preloads(HKEY_CURRENT_USER, "Keyboard Layout\\Preload");
                clean_assembly_item_dups(HKEY_CURRENT_USER, "Software\\Microsoft\\CTF\\SortOrder\\AssemblyItem");
                clean_dead_assemblies(HKEY_CURRENT_USER, "Software\\Microsoft\\CTF\\Assemblies");
        }
}

/// `--cleanup` runs in the installing user's context; `--cleanup-system` runs
/// elevated from the installer. Both purge residue left by older versions.
fn cleanup_residue(system: bool) {
        if system {
                // .DEFAULT picked up tips/preload writes when old code ran
                // elevated (regsvr32, elevated ILOT) — nothing there is legit.
                use windows::Win32::System::Registry::HKEY_USERS;
                sweep_tips_in(HKEY_USERS, ".DEFAULT\\Control Panel\\International\\User Profile", None);
                clean_dead_preloads(HKEY_USERS, ".DEFAULT\\Keyboard Layout\\Preload");
                clean_assembly_item_dups(HKEY_USERS, ".DEFAULT\\Software\\Microsoft\\CTF\\SortOrder\\AssemblyItem");
                clean_dead_assemblies(HKEY_USERS, ".DEFAULT\\Software\\Microsoft\\CTF\\Assemblies");
        } else {
                // Registry-only reconcile — skip the InstallLayoutOrTip
                // language force-add (it can spawn a hidden system dialog
                // when invoked from a spawned process). ensure_input_tip
                // covers it on the normal tray path.
                use windows::Win32::System::Registry::*;
                let cn_active = profile_enabled(0x0804);
                let keep = if cn_active {
                        Some("zh-hans-cn")
                } else if profile_registered(0x0c04) {
                        ensure_hk_tip_entry();
                        Some("zh-hant-hk")
                } else if profile_registered(0x0404) {
                        Some("zh-hant-tw")
                } else {
                        None
                };
                if let Some(keep) = keep {
                        sweep_tips_in(HKEY_CURRENT_USER, "Control Panel\\International\\User Profile", Some(keep));
                }
                clean_dead_preloads(HKEY_CURRENT_USER, "Keyboard Layout\\Preload");
                clean_assembly_item_dups(HKEY_CURRENT_USER, "Software\\Microsoft\\CTF\\SortOrder\\AssemblyItem");
                clean_dead_assemblies(HKEY_CURRENT_USER, "Software\\Microsoft\\CTF\\Assemblies");
                delete_user_dir_value();
        }
}

/// Remove `HKCU\Software\RCantonese\UserDir`. The key stays if an older
/// `Settings` subkey is still there.
fn delete_user_dir_value() {
        use windows::Win32::System::Registry::{RegCloseKey, RegDeleteKeyW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, KEY_READ, KEY_SET_VALUE};
        unsafe {
                let mut key = HKEY::default();
                if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\RCantonese"), None, KEY_READ | KEY_SET_VALUE, &mut key).is_err() {
                        return;
                }
                let _ = RegDeleteValueW(key, w!("UserDir"));
                let mut name = [0u16; 64];
                let mut name_len = name.len() as u32;
                let has_subkey = RegEnumKeyExW(key, 0, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut name_len, None, None, None, None).is_ok();
                name_len = name.len() as u32;
                let has_value = RegEnumValueW(key, 0, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut name_len, None, None, None, None).is_ok();
                let _ = RegCloseKey(key);
                if !has_subkey && !has_value {
                        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, w!("Software\\RCantonese"));
                }
        }
}

/// Whether `...\LanguageProfile\0x0000{langid}\{profile}` exists.
/// The parent lang key is not enough: UnregisterProfile often leaves it
/// behind empty, and treating that as "still zh-HK" makes every login
/// delete the zh-CN tip Settings actually lists.
/// zh-HK additionally keeps 0x0404 as the bridge dummy's host.
fn profile_registered(langid: u16) -> bool {
        use windows::Win32::System::Registry::*;
        unsafe {
                let path = format!(
                        "SOFTWARE\\Microsoft\\CTF\\TIP\\{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}\\LanguageProfile\\{langid:#010x}\\{{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}}\0"
                );
                let wide: Vec<u16> = path.encode_utf16().collect();
                let mut key = HKEY::default();
                let ok = RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        PCWSTR(wide.as_ptr()),
                        Some(0),
                        KEY_READ | KEY_WOW64_64KEY,
                        &mut key,
                ) == ERROR_SUCCESS;
                if ok {
                        let _ = RegCloseKey(key);
                }
                ok
        }
}

/// `Enable == 1` on that langid's profile. A disabled 0x0c04 key left behind
/// after switching to zh-CN must not win over the enabled zh-CN profile.
fn profile_enabled(langid: u16) -> bool {
        use windows::Win32::System::Registry::*;
        unsafe {
                let path = format!(
                        "SOFTWARE\\Microsoft\\CTF\\TIP\\{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}\\LanguageProfile\\{langid:#010x}\\{{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}}\0"
                );
                let wide: Vec<u16> = path.encode_utf16().collect();
                let mut key = HKEY::default();
                if RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        PCWSTR(wide.as_ptr()),
                        Some(0),
                        KEY_READ | KEY_WOW64_64KEY,
                        &mut key,
                ) != ERROR_SUCCESS
                {
                        return false;
                }
                let name: Vec<u16> = "Enable".encode_utf16().chain(Some(0)).collect();
                let mut data = [0u8; 4];
                let mut size = 4u32;
                let ok = RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, Some(data.as_mut_ptr()), Some(&mut size))
                        == ERROR_SUCCESS;
                let _ = RegCloseKey(key);
                ok && size == 4 && u32::from_le_bytes(data) == 1
        }
}

/// Delete `*:{clsid}{guid}` DWORDs from every `<root>\<lang>` key except the
/// `keep` language — entries under other languages surface as extra
/// R-Cantonese rows in the language picker. `keep: None` sweeps all of them
/// (used for the .DEFAULT hive where nothing of ours is legitimate).
fn sweep_tips_in(hive: HKEY, root: &str, keep: Option<&str>) {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        const SUFFIX: &str = "{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";
        unsafe {
                let root_wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
                let mut root_key = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(root_wide.as_ptr()), Some(0), KEY_READ, &mut root_key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut langs = Vec::new();
                let mut i = 0u32;
                loop {
                        let mut buf = [0u16; 128];
                        let mut len = buf.len() as u32;
                        if RegEnumKeyExW(root_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                != ERROR_SUCCESS
                        {
                                break;
                        }
                        langs.push(String::from_utf16_lossy(&buf[..len as usize]));
                        i += 1;
                }
                let _ = RegCloseKey(root_key);
                for lang in langs {
                        if keep.is_some_and(|k| lang.eq_ignore_ascii_case(k)) {
                                continue;
                        }
                        let path = format!("{root}\\{lang}\0");
                        let wide: Vec<u16> = path.encode_utf16().collect();
                        let mut sub = HKEY::default();
                        if RegOpenKeyExW(
                                hive,
                                PCWSTR(wide.as_ptr()),
                                Some(0),
                                KEY_READ | KEY_SET_VALUE,
                                &mut sub,
                        ) != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let mut doomed = Vec::new();
                        let mut j = 0u32;
                        loop {
                                let mut nbuf = [0u16; 256];
                                let mut nlen = nbuf.len() as u32;
                                if RegEnumValueW(sub, j, Some(PWSTR(nbuf.as_mut_ptr())), &mut nlen, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                let name = String::from_utf16_lossy(&nbuf[..nlen as usize]);
                                if name.to_uppercase().ends_with(SUFFIX) {
                                        doomed.push(name);
                                }
                                j += 1;
                        }
                        for name in doomed {
                                let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                                let _ = RegDeleteValueW(sub, PCWSTR(w.as_ptr()));
                        }
                        let _ = RegCloseKey(sub);
                }
        }
}

/// A custom zh-HK KLID — the `[de]`-prefixed range is user/IME-defined; the
/// old IMM32/`0C04:` paths registered `e0000c04`-style ids and ILOT tipped
/// `d0010c04`-style ones, so anything `[de]*0c04` is our residue.
fn is_dead_zhhk_klid(s: &str) -> bool {
        let s = s.to_lowercase();
        s.len() == 8 && (s.starts_with('e') || s.starts_with('d')) && s.ends_with("0c04")
}

/// Delete Preload entries whose data is a custom zh-HK KLID, and the matching
/// Substitutes entries (the substitute chain `d0010c04→00000c04→00000409`
/// keeps a dead zh-HK input source alive in the flyout).
fn clean_dead_preloads(hive: HKEY, path: &str) {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        unsafe {
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                let mut key = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(wide.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut doomed = Vec::new();
                let mut i = 0u32;
                loop {
                        let mut nbuf = [0u16; 64];
                        let mut nlen = nbuf.len() as u32;
                        let mut dbuf = [0u8; 64];
                        let mut dlen = dbuf.len() as u32;
                        if RegEnumValueW(
                                key,
                                i,
                                Some(PWSTR(nbuf.as_mut_ptr())),
                                &mut nlen,
                                None,
                                None,
                                Some(dbuf.as_mut_ptr()),
                                Some(&mut dlen),
                        ) != ERROR_SUCCESS
                        {
                                break;
                        }
                        let u16s = (dlen as usize / 2).min(32);
                        let data = String::from_utf16_lossy(std::slice::from_raw_parts(
                                dbuf.as_ptr() as *const u16,
                                u16s,
                        ));
                        let data = data.trim_end_matches('\0').to_string();
                        let name = String::from_utf16_lossy(&nbuf[..nlen as usize]);
                        if is_dead_zhhk_klid(&data) {
                                doomed.push(name);
                        }
                        i += 1;
                }
                for name in doomed {
                        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                        let _ = RegDeleteValueW(key, PCWSTR(w.as_ptr()));
                }
                let _ = RegCloseKey(key);

                // Substitutes\<klid> entries naming a dead KLID keep the HKL
                // resolvable — delete those too (value data, not the name,
                // decides whether a substitute is needed).
                let subs_path = if hive == HKEY_USERS {
                        ".DEFAULT\\Keyboard Layout\\Substitutes"
                } else {
                        "Keyboard Layout\\Substitutes"
                };
                let wide: Vec<u16> = subs_path.encode_utf16().chain(Some(0)).collect();
                let mut skey = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(wide.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut skey)
                        == ERROR_SUCCESS
                {
                        let mut doomed_subs = Vec::new();
                        let mut i = 0u32;
                        loop {
                                let mut nbuf = [0u16; 64];
                                let mut nlen = nbuf.len() as u32;
                                if RegEnumValueW(skey, i, Some(PWSTR(nbuf.as_mut_ptr())), &mut nlen, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                i += 1;
                                let name = String::from_utf16_lossy(&nbuf[..nlen as usize]);
                                if is_dead_zhhk_klid(&name) {
                                        doomed_subs.push(name);
                                }
                        }
                        for name in doomed_subs {
                                let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                                let _ = RegDeleteValueW(skey, PCWSTR(w.as_ptr()));
                        }
                        let _ = RegCloseKey(skey);
                }
        }
}

/// Langids that still have a live tips DWORD naming our tip — the DWORD
/// prefix (e.g. "0404") selects the profile's langid.
fn live_tip_langids(hive: HKEY) -> std::collections::HashSet<u32> {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        const OUR_TIP: &str = "{D2291A80-84D8-4641-9AB2-BDD1472C846B}{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}";
        let tips_root = if hive == HKEY_USERS {
                ".DEFAULT\\Control Panel\\International\\User Profile"
        } else {
                "Control Panel\\International\\User Profile"
        };
        let mut active_langs = std::collections::HashSet::new();
        unsafe {
                let tr_wide: Vec<u16> = tips_root.encode_utf16().chain(Some(0)).collect();
                let mut tips_key = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(tr_wide.as_ptr()), Some(0), KEY_READ, &mut tips_key)
                        == ERROR_SUCCESS
                {
                        let mut langs = Vec::new();
                        let mut i = 0u32;
                        loop {
                                let mut buf = [0u16; 128];
                                let mut len = buf.len() as u32;
                                if RegEnumKeyExW(tips_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                langs.push(String::from_utf16_lossy(&buf[..len as usize]));
                                i += 1;
                        }
                        let _ = RegCloseKey(tips_key);
                        for lang in langs {
                                let p = format!("{tips_root}\\{lang}\0");
                                let pw: Vec<u16> = p.encode_utf16().collect();
                                let mut lang_key = HKEY::default();
                                if RegOpenKeyExW(hive, PCWSTR(pw.as_ptr()), Some(0), KEY_READ, &mut lang_key)
                                        != ERROR_SUCCESS
                                {
                                        continue;
                                }
                                let mut j = 0u32;
                                loop {
                                        let mut nb = [0u16; 256];
                                        let mut nl = nb.len() as u32;
                                        if RegEnumValueW(lang_key, j, Some(PWSTR(nb.as_mut_ptr())), &mut nl, None, None, None, None)
                                                != ERROR_SUCCESS
                                        {
                                                break;
                                        }
                                        j += 1;
                                        let name = String::from_utf16_lossy(&nb[..nl as usize]);
                                        if name.to_uppercase().ends_with(OUR_TIP) {
                                                if let Some(hex) = name.split(':').next() {
                                                        if let Ok(langid) = u32::from_str_radix(hex, 16) {
                                                                active_langs.insert(langid);
                                                        }
                                                }
                                        }
                                }
                                let _ = RegCloseKey(lang_key);
                        }
                }
        }
        active_langs
}

/// `SortOrder\AssemblyItem\<langid>\<item-guid>\<order>` leaves carry
/// CLSID/Profile — enabling a tips DWORD creates one; deleting the DWORD
/// later leaves the leaf behind as an orphan flyout row. For our CLSID a leaf
/// is legit only while a `{langid}:{clsid}{guid}` DWORD exists under that
/// hive's `User Profile\<lang>`; otherwise (and for dups) delete it.
fn clean_assembly_item_dups(hive: HKEY, root: &str) {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        const OURS: &str = "{D2291A80-84D8-4641-9AB2-BDD1472C846B}";
        unsafe {
                let active_langs = live_tip_langids(hive);

                // Enumerate AssemblyItem\<langid>\<item>\<order>; drop leaves
                // whose langid has no live tips DWORD, dedupe the rest.
                let asm_wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
                let mut asm_key = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(asm_wide.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut asm_key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut lang_roots = Vec::new();
                let mut i = 0u32;
                loop {
                        let mut buf = [0u16; 32];
                        let mut len = buf.len() as u32;
                        if RegEnumKeyExW(asm_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                != ERROR_SUCCESS
                        {
                                break;
                        }
                        lang_roots.push(String::from_utf16_lossy(&buf[..len as usize]));
                        i += 1;
                }
                let _ = RegCloseKey(asm_key);
                for lang_root in lang_roots {
                        let Some(langid) = lang_root
                                .strip_prefix("0x")
                                .or_else(|| lang_root.strip_prefix("0X"))
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                        else {
                                continue;
                        };
                        let lang_active = active_langs.contains(&langid);
                        let lang_path = format!("{root}\\{lang_root}\0");
                        let lw: Vec<u16> = lang_path.encode_utf16().collect();
                        let mut lang_key = HKEY::default();
                        if RegOpenKeyExW(hive, PCWSTR(lw.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut lang_key)
                                != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let mut items = Vec::new();
                        let mut i = 0u32;
                        loop {
                                let mut buf = [0u16; 128];
                                let mut len = buf.len() as u32;
                                if RegEnumKeyExW(lang_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                items.push(String::from_utf16_lossy(&buf[..len as usize]));
                                i += 1;
                        }
                        let mut seen = false;
                        let mut doomed: Vec<(String, String)> = Vec::new();
                        for item in items {
                                // Each <item> is a shared container — sibling
                                // <order> leaves may belong to other IMEs.
                                // Delete only OUR leaves, never the item key.
                                let item_path = format!("{root}\\{lang_root}\\{item}\0");
                                let iw: Vec<u16> = item_path.encode_utf16().collect();
                                let mut item_key = HKEY::default();
                                if RegOpenKeyExW(hive, PCWSTR(iw.as_ptr()), Some(0), KEY_READ, &mut item_key)
                                        != ERROR_SUCCESS
                                {
                                        continue;
                                }
                                let mut j = 0u32;
                                let mut item_orders = Vec::new();
                                loop {
                                        let mut buf = [0u16; 32];
                                        let mut len = buf.len() as u32;
                                        if RegEnumKeyExW(item_key, j, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                                != ERROR_SUCCESS
                                        {
                                                break;
                                        }
                                        item_orders.push(String::from_utf16_lossy(&buf[..len as usize]));
                                        j += 1;
                                }
                                for order in item_orders {
                                        let op = format!("{root}\\{lang_root}\\{item}\\{order}\0");
                                        let ow: Vec<u16> = op.encode_utf16().collect();
                                        let mut okey = HKEY::default();
                                        if RegOpenKeyExW(hive, PCWSTR(ow.as_ptr()), Some(0), KEY_READ, &mut okey)
                                                != ERROR_SUCCESS
                                        {
                                                continue;
                                        }
                                        let mut vbuf = [0u16; 64];
                                        let mut vlen = (vbuf.len() * 2) as u32;
                                        let vname: Vec<u16> = "CLSID".encode_utf16().chain(Some(0)).collect();
                                        let ok = RegQueryValueExW(
                                                okey,
                                                PCWSTR(vname.as_ptr()),
                                                None,
                                                None,
                                                Some(vbuf.as_mut_ptr() as *mut u8),
                                                Some(&mut vlen),
                                        ) == ERROR_SUCCESS;
                                        let _ = RegCloseKey(okey);
                                        if ok {
                                                let text = String::from_utf16_lossy(&vbuf[..(vlen as usize / 2).saturating_sub(1)]);
                                                if text.eq_ignore_ascii_case(OURS) {
                                                        if !lang_active || seen {
                                                                doomed.push((item.clone(), order));
                                                        } else {
                                                                seen = true;
                                                        }
                                                }
                                        }
                                }
                                let _ = RegCloseKey(item_key);
                        }
                        for (item, order) in doomed {
                                let ip = format!("{root}\\{lang_root}\\{item}\0");
                                let iw: Vec<u16> = ip.encode_utf16().collect();
                                let mut ik = HKEY::default();
                                if RegOpenKeyExW(hive, PCWSTR(iw.as_ptr()), Some(0), KEY_ALL_ACCESS, &mut ik)
                                        == ERROR_SUCCESS
                                {
                                        let w: Vec<u16> = order.encode_utf16().chain(Some(0)).collect();
                                        let _ = RegDeleteTreeW(ik, PCWSTR(w.as_ptr()));
                                        let _ = RegCloseKey(ik);
                                }
                        }
                        let _ = RegCloseKey(lang_key);
                }
        }
}

/// `CTF\Assemblies\<langid>\<item-guid>` is a *separate* cache from
/// AssemblyItem — each leaf binds our CLSID to a KeyboardLayout HKL. Leaves
/// survive their tips DWORD just like AssemblyItem leaves and produce the
/// same phantom flyout row (e.g. a 0x0c04 binding left by the old `0C04:`
/// path). Same rule: ours + no `{langid}:{ours}` tips DWORD = orphan.
fn clean_dead_assemblies(hive: HKEY, root: &str) {
        use windows::Win32::System::Registry::*;
        use windows::core::PWSTR;
        const OURS: &str = "{D2291A80-84D8-4641-9AB2-BDD1472C846B}";
        unsafe {
                let active_langs = live_tip_langids(hive);
                let asm_wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
                let mut asm_key = HKEY::default();
                if RegOpenKeyExW(hive, PCWSTR(asm_wide.as_ptr()), Some(0), KEY_READ | KEY_SET_VALUE, &mut asm_key)
                        != ERROR_SUCCESS
                {
                        return;
                }
                let mut lang_roots = Vec::new();
                let mut i = 0u32;
                loop {
                        let mut buf = [0u16; 32];
                        let mut len = buf.len() as u32;
                        if RegEnumKeyExW(asm_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                != ERROR_SUCCESS
                        {
                                break;
                        }
                        lang_roots.push(String::from_utf16_lossy(&buf[..len as usize]));
                        i += 1;
                }
                let _ = RegCloseKey(asm_key);
                for lang_root in lang_roots {
                        let Some(langid) = lang_root
                                .strip_prefix("0x")
                                .or_else(|| lang_root.strip_prefix("0X"))
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                        else {
                                continue;
                        };
                        let lang_active = active_langs.contains(&langid);
                        let lang_path = format!("{root}\\{lang_root}\0");
                        let lw: Vec<u16> = lang_path.encode_utf16().collect();
                        let mut lang_key = HKEY::default();
                        if RegOpenKeyExW(hive, PCWSTR(lw.as_ptr()), Some(0), KEY_READ, &mut lang_key)
                                != ERROR_SUCCESS
                        {
                                continue;
                        }
                        let mut items = Vec::new();
                        let mut i = 0u32;
                        loop {
                                let mut buf = [0u16; 128];
                                let mut len = buf.len() as u32;
                                if RegEnumKeyExW(lang_key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                                        != ERROR_SUCCESS
                                {
                                        break;
                                }
                                items.push(String::from_utf16_lossy(&buf[..len as usize]));
                                i += 1;
                        }
                        let _ = RegCloseKey(lang_key);
                        let mut seen = false;
                        let mut doomed = Vec::new();
                        for item in items {
                                // Read the leaf's Default value — it's the
                                // bound CLSID. Only delete OUR bindings.
                                let ip = format!("{root}\\{lang_root}\\{item}\0");
                                let iw: Vec<u16> = ip.encode_utf16().collect();
                                let mut ikey = HKEY::default();
                                if RegOpenKeyExW(hive, PCWSTR(iw.as_ptr()), Some(0), KEY_READ, &mut ikey)
                                        != ERROR_SUCCESS
                                {
                                        continue;
                                }
                                let mut vbuf = [0u16; 64];
                                let mut vlen = (vbuf.len() * 2) as u32;
                                let vname: Vec<u16> = "Default".encode_utf16().chain(Some(0)).collect();
                                let ok = RegQueryValueExW(
                                        ikey,
                                        PCWSTR(vname.as_ptr()),
                                        None,
                                        None,
                                        Some(vbuf.as_mut_ptr() as *mut u8),
                                        Some(&mut vlen),
                                ) == ERROR_SUCCESS;
                                let _ = RegCloseKey(ikey);
                                if ok {
                                        let text = String::from_utf16_lossy(&vbuf[..(vlen as usize / 2).saturating_sub(1)]);
                                        if text.eq_ignore_ascii_case(OURS) {
                                                if !lang_active || seen {
                                                        doomed.push(item);
                                                } else {
                                                        seen = true;
                                                }
                                        }
                                }
                        }
                        for item in doomed {
                                let ip = format!("{root}\\{lang_root}\0");
                                let iw: Vec<u16> = ip.encode_utf16().collect();
                                let mut ik = HKEY::default();
                                if RegOpenKeyExW(hive, PCWSTR(iw.as_ptr()), Some(0), KEY_ALL_ACCESS, &mut ik)
                                        == ERROR_SUCCESS
                                {
                                        let w: Vec<u16> = item.encode_utf16().chain(Some(0)).collect();
                                        let _ = RegDeleteTreeW(ik, PCWSTR(w.as_ptr()));
                                        let _ = RegCloseKey(ik);
                                }
                        }
                }
        }
}

/// Write `0404:{clsid}{guid}` into zh-Hant-HK's input-method list —
/// HKCU\Control Panel\International\User Profile\zh-Hant-HK holds DWORD
/// ordering values named by tip string. Idempotent.
fn ensure_hk_tip_entry() {
        use windows::Win32::System::Registry::*;
        unsafe {
                let key_path: Vec<u16> = "Control Panel\\International\\User Profile\\zh-Hant-HK"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                let mut key = HKEY::default();
                if RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(key_path.as_ptr()), &mut key) != ERROR_SUCCESS {
                        log_error("ensure_hk_tip_entry: key open failed");
                        return;
                }
                // Drop the stale 0C04 entry left by <=0.9.2 installs — the
                // flyout would otherwise show two R-Cantonese entries under
                // zh-HK, and the 0c04 path has no bridge dummy behind it.
                let stale: Vec<u16> = INPUT_TIP_0C04.encode_utf16().chain(Some(0)).collect();
                let _ = RegDeleteValueW(key, PCWSTR(stale.as_ptr()));
                let name: Vec<u16> = INPUT_TIP.encode_utf16().chain(Some(0)).collect();
                let mut exists = [0u8; 4];
                let mut size = 4u32;
                if RegQueryValueExW(
                        key,
                        PCWSTR(name.as_ptr()),
                        None,
                        None,
                        Some(exists.as_mut_ptr()),
                        Some(&mut size),
                ) == ERROR_SUCCESS
                {
                        let _ = RegCloseKey(key);
                        return;
                }
                // Next ordering index = max existing DWORD value + 1.
                let mut index = 1u32;
                let mut i = 0u32;
                loop {
                        let mut nbuf = [0u16; 128];
                        let mut nlen = nbuf.len() as u32;
                        let mut data = [0u8; 4];
                        let mut dlen = 4u32;
                        if RegEnumValueW(
                                key,
                                i,
                                Some(PWSTR(nbuf.as_mut_ptr())),
                                &mut nlen,
                                None,
                                None,
                                Some(data.as_mut_ptr()),
                                Some(&mut dlen),
                        ) != ERROR_SUCCESS
                        {
                                break;
                        }
                        i += 1;
                        if dlen == 4 {
                                index = index.max(u32::from_le_bytes(data) + 1);
                        }
                }
                let ok = RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_DWORD, Some(&index.to_le_bytes()));
                let _ = RegCloseKey(key);
                if ok != ERROR_SUCCESS {
                        log_error("ensure_hk_tip_entry: write failed");
                }
        }
}

/// Attach `{langid}:{clsid}{guid}` with InstallLayoutOrTip so Settings lists
/// the IME under that language. Unlike zh-HK, these langids match the tip
/// prefix, so ILOT is allowed. Also adds the language's US keyboard when the
/// language itself is missing, or the tip has nothing to hang on.
fn ensure_lang_tip(langid: u16, tags: &[&str]) {
        unsafe {
                if !user_has_language_tag(tags) {
                        let kb: Vec<u16> = format!("{langid:04X}:00000409").encode_utf16().chain(Some(0)).collect();
                        if !InstallLayoutOrTip(PCWSTR(kb.as_ptr()), 0).as_bool() {
                                log_error(&format!("InstallLayoutOrTip failed (language {langid:#06x})"));
                        }
                }
                let tip: Vec<u16> = format!(
                        "{langid:04X}:{{D2291A80-84D8-4641-9AB2-BDD1472C846B}}{{83955C0E-2C09-47A5-BCF3-F2B98E11EE8B}}"
                )
                .encode_utf16()
                .chain(Some(0))
                .collect();
                if !InstallLayoutOrTip(PCWSTR(tip.as_ptr()), 0).as_bool() {
                        log_error(&format!("InstallLayoutOrTip failed (tip {langid:#06x})"));
                }
        }
}

/// Whether any of `tags` is in the user's profile language list —
/// HKCU\Control Panel\International\User Profile\Languages (REG_MULTI_SZ).
fn user_has_language_tag(tags: &[&str]) -> bool {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_MULTI_SZ, RegGetValueW};
        unsafe {
                let key_path: Vec<u16> = "Control Panel\\International\\User Profile"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                let value: Vec<u16> = "Languages".encode_utf16().chain(Some(0)).collect();
                let mut size = 0u32;
                if RegGetValueW(
                        HKEY_CURRENT_USER,
                        PCWSTR(key_path.as_ptr()),
                        PCWSTR(value.as_ptr()),
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
                        PCWSTR(key_path.as_ptr()),
                        PCWSTR(value.as_ptr()),
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
                        .any(|s| tags.iter().any(|tag| s == *tag))
        }
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

/// Same record Weasel writes as `RimeUserDir`: the expanded absolute
/// directory, so a packaged host does not follow its private `%LOCALAPPDATA%`.
fn remember_user_dir() {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegSetValueExW, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ};
        let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
        let dir = std::path::PathBuf::from(local).join("RCantonese");
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
                let mut key = HKEY::default();
                if RegCreateKeyExW(HKEY_CURRENT_USER, w!("Software\\RCantonese"), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None).is_err() {
                        return;
                }
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                let _ = RegSetValueExW(key, w!("UserDir"), None, REG_SZ, Some(bytes));
                let _ = RegCloseKey(key);
        }
}

fn main() {
        // Headless cleanup modes invoked by the installer post-install —
        // run before the single-instance check so they work while a tray is
        // already up.
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a.eq_ignore_ascii_case("--cleanup")) {
                cleanup_residue(false);
                return;
        }
        if args.iter().any(|a| a.eq_ignore_ascii_case("--cleanup-system")) {
                cleanup_residue(true);
                return;
        }
        remember_user_dir();
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
                // Enable our input tip — idempotent; repairs a registration
                // where the elevated regsvr32 couldn't reach the user hive.
                ensure_input_tip();
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
                settings_server::start();
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

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        WM_TRAY_PING => LRESULT(TRAY_MAGIC),
                        WM_TRAY_QUIT => {
                                let _ = DestroyWindow(hwnd);
                                PostQuitMessage(0);
                                LRESULT(0)
                        }
                        _ => DefWindowProcW(hwnd, msg, _wparam, lparam),
                }
        }
}
