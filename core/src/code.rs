use alloc::vec::Vec;
use core::ops::Deref;
use primitive_types::H160;

/// Contract code
#[derive(Clone, Debug)]
#[cfg_attr(feature = "with-codec", derive(codec::Encode, codec::Decode))]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Code {
    /// Account reference
    AccountRef {
        /// pointer to the code data
        #[cfg_attr(feature = "with-serde", serde(skip))]
        #[cfg_attr(feature = "with-serde", serde(default = "core::ptr::null"))]
        ptr: *const u8,

        /// code size
        #[cfg_attr(feature = "with-serde", serde(skip))]
        len: usize,

        /// code account
        account: H160,
    },
    /// Owned code
    Vec {
        /// Code storage
        #[cfg_attr(feature = "with-serde", serde(with = "serde_bytes"))]
        code: Vec<u8>,
    },
}

impl Deref for Code {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Code::AccountRef { ptr, len, .. } => unsafe { core::slice::from_raw_parts(*ptr, *len) },
            Code::Vec { code } => &code[..],
        }
    }
}
