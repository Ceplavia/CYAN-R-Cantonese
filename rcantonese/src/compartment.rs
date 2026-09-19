// Compartment helpers — port of Compartment.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::*;

const S_FALSE_HR: HRESULT = HRESULT(0x00000001);
const E_INVALID: HRESULT = HRESULT(0x80070057u32 as i32);

#[derive(Clone)]
pub struct Compartment {
        punk: IUnknown,
        client_id: u32,
        guid: GUID,
}

impl Compartment {
        pub fn new(punk: &IUnknown, client_id: u32, guid: GUID) -> Self {
                Self {
                        punk: punk.clone(),
                        client_id,
                        guid,
                }
        }

        fn get_compartment(&self) -> Result<ITfCompartment> {
                let mgr: ITfCompartmentMgr = self.punk.cast()?;
                unsafe { mgr.GetCompartment(&self.guid) }
        }

        fn get_value(&self) -> Result<Option<i32>> {
                let compartment = self.get_compartment()?;
                let value = unsafe { compartment.GetValue()? };
                Ok(variant_to_i32(&value))
        }

        pub fn get_bool(&self) -> Result<bool> {
                match self.get_value()? {
                        Some(v) => Ok(v != 0),
                        None => Err(Error::from_hresult(S_FALSE_HR)),
                }
        }

        pub fn get_dword(&self) -> Result<u32> {
                match self.get_value()? {
                        Some(v) => Ok(v as u32),
                        None => Err(Error::from_hresult(S_FALSE_HR)),
                }
        }

        pub fn set_bool(&self, flag: bool) -> Result<()> {
                let compartment = self.get_compartment()?;
                let value = VARIANT::from(if flag { 1i32 } else { 0i32 });
                unsafe { compartment.SetValue(self.client_id, &value) }
        }

        pub fn set_dword(&self, value: u32) -> Result<()> {
                let compartment = self.get_compartment()?;
                let v = VARIANT::from(value as i32);
                unsafe { compartment.SetValue(self.client_id, &v) }
        }

        pub fn clear(&self) -> Result<()> {
                if self.guid == GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
                        return Err(Error::from_hresult(S_FALSE_HR));
                }
                let mgr: ITfCompartmentMgr = self.punk.cast()?;
                unsafe { mgr.ClearCompartment(self.client_id, &self.guid) }
        }
}

fn variant_to_i32(value: &VARIANT) -> Option<i32> {
        // VT_I4 == 3
        unsafe {
                let inner = &value.Anonymous.Anonymous;
                if inner.vt.0 == 3 {
                        Some(inner.Anonymous.lVal)
                } else {
                        None
                }
        }
}
