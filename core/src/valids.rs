use alloc::{vec,vec::Vec};

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
	#[must_use]
	pub fn new(code: &[u8]) -> Self {
		let mut valids: Vec<u8> = vec![0; code.len()];

		let mut i = 0;
		while i < code.len() {
			let opcode = code[i];
			match opcode {
				0x5b => { // Jump Dest
					valids[i] = 1_u8;
				},
				0x60..=0x7f => { // Push
					i += (opcode as usize) - 0x60 + 1;
				},
				_ => {}
			}

			i += 1;
		}

		Valids{ data: valids }
	}

	/// Get the length of the valid mapping. This is the same as the
	/// code bytes.
	#[inline]
	#[must_use]
	pub fn len(&self) -> usize {
		self.data.len()
	}

	/// Returns true if the valids list is empty
	#[inline]
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Returns `true` if the position is a valid jump destination. If
	/// not, returns `false`.
	#[must_use]
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
