// Deployed-binary check — loads the INSTALLED .ime and exercises its
// exports in this process. Kept in its own test binary because the loaded
// dll's DllMain registers our window-class names bound to ITS wndprocs;
// sharing a process with the in-process host test would make new windows
// run old code against new state layouts.

#[test]
fn ime_inquire_fills_info() {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::Win32::UI::Input::Ime::*;
        use windows::core::BOOL;
        use windows::core::PCSTR;
        let path = "C:\\Windows\\System32\\r-cantonese.ime".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let h = unsafe { LoadLibraryW(windows::core::PCWSTR(path.as_ptr())) }.unwrap();
        let inq: unsafe extern "system" fn(*mut IMEINFO, *mut u16, u32) -> BOOL = unsafe {
                std::mem::transmute(GetProcAddress(h, PCSTR(b"ImeInquire\0".as_ptr())).unwrap())
        };
        let mut info = IMEINFO::default();
        let mut cls = [0u16; 64];
        let ok = unsafe { inq(&mut info, cls.as_mut_ptr(), 0) };
        assert!(ok.as_bool(), "ImeInquire returned false");
        println!("prop={:#x} conv={:#x} ui={:#x} scs={:#x} sel={:#x} priv={} class={:?}",
                info.fdwProperty, info.fdwConversionCaps, info.fdwUICaps, info.fdwSCSCaps,
                info.fdwSelectCaps, info.dwPrivateDataSize,
                String::from_utf16_lossy(&cls[..cls.iter().position(|&c| c==0).unwrap_or(64)]));
}
