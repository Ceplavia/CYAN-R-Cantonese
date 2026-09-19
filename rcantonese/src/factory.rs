// IClassFactory — port of ClassFactory.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::System::Com::*;

use crate::globals;
use crate::service::RCantoneseService;

const CLASS_E_NOAGGREGATION: HRESULT = HRESULT(0x80040110u32 as i32);
const E_NOINTERFACE: HRESULT = HRESULT(0x80004002u32 as i32);

#[implement(IClassFactory)]
pub struct RCantoneseClassFactory;

impl IClassFactory_Impl for RCantoneseClassFactory_Impl {
        fn CreateInstance(
                &self,
                punkouter: Ref<'_, IUnknown>,
                riid: *const GUID,
                ppvobject: *mut *mut core::ffi::c_void,
        ) -> Result<()> {
                globals::guarded("CreateInstance", || self.create_instance_inner(punkouter, riid, ppvobject))
        }

        fn LockServer(&self, flock: BOOL) -> Result<()> {
                if flock.as_bool() {
                        globals::dll_add_ref();
                } else {
                        globals::dll_release();
                }
                Ok(())
        }
}

impl RCantoneseClassFactory_Impl {
        fn create_instance_inner(
                &self,
                punkouter: Ref<'_, IUnknown>,
                riid: *const GUID,
                ppvobject: *mut *mut core::ffi::c_void,
        ) -> Result<()> {
                globals::log("RCantoneseClassFactory::CreateInstance");
                if ppvobject.is_null() {
                        return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
                }
                unsafe { *ppvobject = std::ptr::null_mut() };
                if !punkouter.is_null() {
                        return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
                }
                let service: IUnknown = RCantoneseService::new().into();
                globals::dll_add_ref();
                let mut pv: *mut core::ffi::c_void = std::ptr::null_mut();
                let hr = unsafe { service.query(riid, &mut pv) };
                if hr.is_ok() {
                        unsafe { *ppvobject = pv };
                        Ok(())
                } else {
                        globals::dll_release();
                        Err(Error::from_hresult(E_NOINTERFACE))
                }
        }
}
