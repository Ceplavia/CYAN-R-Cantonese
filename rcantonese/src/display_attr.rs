// Display attributes — port of DisplayAttributeInfo.cpp / EnumDisplayAttributeInfo.cpp.
#![allow(dead_code)]

use std::sync::Mutex;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::TextServices::*;

use crate::globals;

fn input_display_attribute() -> TF_DISPLAYATTRIBUTE {
        const BLUE: COLORREF = COLORREF(0x00CE6700); // RGB(0, 103, 206)
        TF_DISPLAYATTRIBUTE {
                crText: TF_DA_COLOR {
                        r#type: TF_CT_COLORREF,
                        Anonymous: TF_DA_COLOR_0 { cr: BLUE },
                },
                crBk: TF_DA_COLOR {
                        r#type: TF_CT_NONE,
                        Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
                },
                lsStyle: TF_LS_DOT,
                fBoldLine: BOOL(0),
                crLine: TF_DA_COLOR {
                        r#type: TF_CT_COLORREF,
                        Anonymous: TF_DA_COLOR_0 { cr: BLUE },
                },
                bAttr: TF_ATTR_INPUT,
        }
}

fn converted_display_attribute() -> TF_DISPLAYATTRIBUTE {
        TF_DISPLAYATTRIBUTE {
                crText: TF_DA_COLOR {
                        r#type: TF_CT_COLORREF,
                        Anonymous: TF_DA_COLOR_0 { cr: COLORREF(0x00FFFFFF) },
                },
                crBk: TF_DA_COLOR {
                        r#type: TF_CT_COLORREF,
                        Anonymous: TF_DA_COLOR_0 { cr: COLORREF(0x00FFFF00) },
                },
                lsStyle: TF_LS_NONE,
                fBoldLine: BOOL(0),
                crLine: TF_DA_COLOR {
                        r#type: TF_CT_NONE,
                        Anonymous: TF_DA_COLOR_0 { nIndex: 0 },
                },
                bAttr: TF_ATTR_TARGET_CONVERTED,
        }
}

#[implement(ITfDisplayAttributeInfo)]
pub struct DisplayAttributeInfo {
        guid: GUID,
        is_input: bool,
}

impl DisplayAttributeInfo {
        pub fn new(guid: GUID, is_input: bool) -> Self {
                Self { guid, is_input }
        }
}

impl ITfDisplayAttributeInfo_Impl for DisplayAttributeInfo_Impl {
        fn GetGUID(&self) -> Result<GUID> {
                Ok(self.guid)
        }

        fn GetDescription(&self) -> Result<BSTR> {
                if self.is_input {
                        Ok(BSTR::from("R-Cantonese Text Service Display Attribute Input"))
                } else {
                        Ok(BSTR::from("R-Cantonese Text Service Display Attribute Converted"))
                }
        }

        fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
                if pda.is_null() {
                        return Err(Error::from_hresult(E_INVALIDARG));
                }
                unsafe {
                        *pda = if self.is_input {
                                input_display_attribute()
                        } else {
                                converted_display_attribute()
                        };
                }
                Ok(())
        }

        fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
                Err(Error::from_hresult(E_NOTIMPL))
        }

        fn Reset(&self) -> Result<()> {
                let attr = if self.is_input {
                        input_display_attribute()
                } else {
                        converted_display_attribute()
                };
                self.SetAttributeInfo(&attr)
        }
}

#[implement(IEnumTfDisplayAttributeInfo)]
pub struct DisplayAttributeEnum {
        index: Mutex<usize>,
}

impl DisplayAttributeEnum {
        pub fn new() -> Self {
                Self { index: Mutex::new(0) }
        }

        fn guids() -> [GUID; 2] {
                [globals::GUID_DISPLAY_ATTRIBUTE_INPUT, globals::GUID_DISPLAY_ATTRIBUTE_CONVERTED]
        }

        fn make_info(index: usize) -> Option<ITfDisplayAttributeInfo> {
                match index {
                        0 => Some(DisplayAttributeInfo::new(globals::GUID_DISPLAY_ATTRIBUTE_INPUT, true).into()),
                        1 => Some(DisplayAttributeInfo::new(globals::GUID_DISPLAY_ATTRIBUTE_CONVERTED, false).into()),
                        _ => None,
                }
        }
}

impl IEnumTfDisplayAttributeInfo_Impl for DisplayAttributeEnum_Impl {
        fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
                let index = *self.index.lock().unwrap_or_else(|e| e.into_inner());
                let cloned = DisplayAttributeEnum { index: Mutex::new(index) };
                Ok(cloned.into())
        }

        fn Next(&self, ulcount: u32, rginfo: *mut Option<ITfDisplayAttributeInfo>, pcfetched: *mut u32) -> Result<()> {
                if rginfo.is_null() {
                        return Err(Error::from_hresult(E_INVALIDARG));
                }
                let mut fetched = 0u32;
                let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
                for i in 0..ulcount as usize {
                        match DisplayAttributeEnum::make_info(*index) {
                                Some(info) => {
                                        unsafe { *rginfo.add(i) = Some(info) };
                                        *index += 1;
                                        fetched += 1;
                                }
                                None => break,
                        }
                }
                if !pcfetched.is_null() {
                        unsafe { *pcfetched = fetched };
                }
                if fetched == ulcount {
                        Ok(())
                } else {
                        Err(Error::from_hresult(S_FALSE))
                }
        }

        fn Reset(&self) -> Result<()> {
                *self.index.lock().unwrap_or_else(|e| e.into_inner()) = 0;
                Ok(())
        }

        fn Skip(&self, ulcount: u32) -> Result<()> {
                let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
                *index = (*index + ulcount as usize).min(2);
                Ok(())
        }
}
