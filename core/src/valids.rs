use alloc::vec::Vec;
use crate::Opcode;

/// Mapping of valid jump destination from code.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "with-codec", derive(codec::Encode, codec::Decode))]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Valids{
	#[cfg_attr(feature = "with-serde", serde(with = "serde_bytes"))]
	data: Vec<u8>
}

impl Valids {
	/// Create a new valid mapping from given code bytes.
	pub fn new(code: &[u8]) -> Self {
		let mut valids: Vec<u8> = Vec::with_capacity(code.len());
		valids.resize(code.len(), 0u8);

		let mut i = 0;
		while i < code.len() {
			let opcode = Opcode::parse(code[i]);
			if let Ok(opcode) = opcode {
				if opcode == Opcode::JumpDest {
					valids[i] = 1;
					i += 1;
				} else if let Some(v) = opcode.is_push() {
					i += v as usize + 1;
				} else {
					i += 1;
				}
			} else {
				i += 1;
			}
		}

		Valids{ data: valids }
	}

	/// Get the length of the valid mapping. This is the same as the
	/// code bytes.
	#[inline]
	pub fn len(&self) -> usize {
		self.data.len()
	}

	/// Returns true if the valids list is empty
	#[inline]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Returns `true` if the position is a valid jump destination. If
	/// not, returns `false`.
	pub fn is_valid(&self, position: usize) -> bool {
		if position >= self.data.len() {
			return false;
		}

		if self.data[position] == 0 {
			return false;
		}

		true
	}
}
