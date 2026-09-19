// Stub for rcantonese/src/variants.rs — CharacterVariant only.
#![allow(dead_code)]

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharacterVariant {
        Traditional = 1,
        HongKong = 2,
        Taiwan = 3,
        Simplified = 4,
}

impl CharacterVariant {
        pub fn from_raw(value: u32) -> Self {
                match value {
                        2 => Self::HongKong,
                        3 => Self::Taiwan,
                        4 => Self::Simplified,
                        _ => Self::Traditional,
                }
        }
}
