// R-Cantonese — Cantonese input method for Windows, implemented in Rust.
// TSF (Text Services Framework) in-process COM DLL.

mod candidate;
mod compartment;
mod composition;
mod config;
mod converter;
mod db;
mod display_attr;
mod engine;
mod extra;
mod factory;
mod globals;
mod keys;
mod keytable;
mod langbar;
mod memory;
mod phrases;
mod pinyin;
mod processor;
mod punctuation;
mod register;
mod segmenter;
mod service;
mod settings;
mod shapes;
mod strings;
mod tray;
mod types;
mod variants;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::core::{BOOL, GUID, HRESULT};

#[unsafe(no_mangle)]
extern "system" fn DllMain(hinstance: HINSTANCE, reason: u32, _reserved: *const core::ffi::c_void) -> BOOL {
        if reason == DLL_PROCESS_ATTACH {
                globals::DLL_INSTANCE.store(hinstance.0 as isize, std::sync::atomic::Ordering::Relaxed);
        } else if reason == DLL_PROCESS_DETACH {
        }
        BOOL(1)
}

/// Port of DllMain.cpp DllGetClassObject.
#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut core::ffi::c_void) -> HRESULT {
        globals::guarded_value("DllGetClassObject", HRESULT(0x80004005u32 as i32), || {
                if ppv.is_null() || rclsid.is_null() || riid.is_null() {
                        return HRESULT(0x80070057u32 as i32); // E_INVALIDARG
                }
                unsafe {
                        *ppv = std::ptr::null_mut();
                        if *rclsid != globals::CLSID_RCANTONESE {
                                return HRESULT(0x80040111u32 as i32); // CLASS_E_CLASSNOTAVAILABLE
                        }
                        let factory: windows::Win32::System::Com::IClassFactory = factory::RCantoneseClassFactory.into();
                        let mut out: *mut core::ffi::c_void = std::ptr::null_mut();
                        let hr = windows::core::Interface::query(&factory, riid, &mut out);
                        if hr.is_ok() {
                                *ppv = out;
                        }
                        hr
                }
        })
}

/// Port of DllMain.cpp DllCanUnloadNow.
#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
        globals::guarded_value("DllCanUnloadNow", HRESULT(0x00000001), || {
                if globals::dll_ref_count() <= 0 {
                        HRESULT(0)
                } else {
                        HRESULT(0x00000001)
                }
        })
}

/// Port of DllRegisterServer.
#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
        globals::guarded_value("DllRegisterServer", HRESULT(0x80004005u32 as i32), || {
                let ok = register::register_server() && register::register_profiles() && register::register_categories();
                register::register_tray_autostart();
                if ok {
                        HRESULT(0)
                } else {
                        HRESULT(0x80004005u32 as i32) // E_FAIL
                }
        })
}

/// Port of DllUnregisterServer.
#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
        globals::guarded_value("DllUnregisterServer", HRESULT(0x80004005u32 as i32), || {
                register::unregister_profiles();
                register::unregister_categories();
                register::unregister_server();
                register::unregister_tray_autostart();
                HRESULT(0)
        })
}

// ---------------------------------------------------------------------
// COM smoke tests — exercise DllGetClassObject -> CreateInstance -> QI
// without registering the DLL (registry writes need admin rights).
// ---------------------------------------------------------------------

#[cfg(test)]
mod com_tests {
        use super::*;
        use windows::core::Interface;
        use windows::Win32::System::Com::IClassFactory;
        use windows::Win32::UI::TextServices::*;

        #[test]
        fn class_object_returns_factory() {
                let mut pv: *mut core::ffi::c_void = std::ptr::null_mut();
                let hr = DllGetClassObject(&globals::CLSID_RCANTONESE, &IClassFactory::IID, &mut pv);
                assert!(hr.is_ok(), "DllGetClassObject failed: {hr:?}");
                assert!(!pv.is_null());
                let factory = unsafe { IClassFactory::from_raw(pv) };
                drop(factory);
        }

        #[test]
        fn create_instance_yields_text_service() {
                let mut pv: *mut core::ffi::c_void = std::ptr::null_mut();
                let hr = DllGetClassObject(&globals::CLSID_RCANTONESE, &IClassFactory::IID, &mut pv);
                assert!(hr.is_ok());
                let factory = unsafe { IClassFactory::from_raw(pv) };
                let tip: ITfTextInputProcessor = unsafe { factory.CreateInstance(None) }.expect("CreateInstance");
                let _display: ITfFunctionProvider = tip.cast().expect("cast to ITfFunctionProvider");
                let _display_attr_provider: ITfDisplayAttributeProvider = tip.cast().expect("cast to ITfDisplayAttributeProvider");
        }

        #[test]
        fn bad_clsid_fails() {
                let mut pv: *mut core::ffi::c_void = std::ptr::null_mut();
                let hr = DllGetClassObject(&GUID::zeroed(), &IClassFactory::IID, &mut pv);
                assert!(hr.is_err());
        }
}

