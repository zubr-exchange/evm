use primitive_types::{H256, U256};
use alloc::vec::Vec;
use crate::ExitError;

/// EVM stack.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "with-codec", derive(codec::Encode, codec::Decode))]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Stack {
	
	data: Vec<U256>,
	limit: usize,
}

impl Stack {
	/// Create a new stack with given limit.
	pub fn new(limit: usize) -> Self {
		Self {
			data: Vec::new(),
			limit,
		}
	}

	/// Stack limit.
	pub fn limit(&self) -> usize {
		self.limit
	}

	/// Stack length.
	pub fn len(&self) -> usize {
		self.data.len()
	}

	/// Pop a value from the stack. If the stack is already empty, returns the
	/// `StackUnderflow` error.
	pub fn pop(&mut self) -> Result<H256, ExitError> {
		self.data.pop()
			.map(|d| {
				let mut value = H256::default();
				d.to_big_endian(&mut value[..]);
				value
			})
			.ok_or(ExitError::StackUnderflow)
	}

	/// Push a new value into the stack. If it will exceed the stack limit,
	/// returns `StackOverflow` error and leaves the stack unchanged.
	pub fn push(&mut self, value: H256) -> Result<(), ExitError> {
		if self.data.len() + 1 > self.limit {
			return Err(ExitError::StackOverflow)
		}
		self.data.push(U256::from_big_endian(&value[..]));
		Ok(())
	}

	/// Pop a value from the stack. If the stack is already empty, returns the
	/// `StackUnderflow` error.
	pub fn pop_u256(&mut self) -> Result<U256, ExitError> {
		self.data.pop().ok_or(ExitError::StackUnderflow)
	}

	/// Push a new value into the stack. If it will exceed the stack limit,
	/// returns `StackOverflow` error and leaves the stack unchanged.
	pub fn push_u256(&mut self, value: U256) -> Result<(), ExitError> {
		if self.data.len() + 1 > self.limit {
			return Err(ExitError::StackOverflow)
		}
		self.data.push(value);
		Ok(())
	}

	/// Peek a value at given index for the stack, where the top of
	/// the stack is at index `0`. If the index is too large,
	/// `StackError::Underflow` is returned.
	pub fn peek(&self, no_from_top: usize) -> Result<U256, ExitError> {
		if self.data.len() > no_from_top {
			Ok(self.data[self.data.len() - no_from_top - 1])
		} else {
			Err(ExitError::StackUnderflow)
		}
	}

	/// Set a value at given index for the stack, where the top of the
	/// stack is at index `0`. If the index is too large,
	/// `StackError::Underflow` is returned.
	pub fn set(&mut self, no_from_top: usize, val: U256) -> Result<(), ExitError> {
		if self.data.len() > no_from_top {
			let len = self.data.len();
			self.data[len - no_from_top - 1] = val;
			Ok(())
		} else {
			Err(ExitError::StackUnderflow)
		}
	}

	/// Dupplicate a value at given index
	pub fn dup(&mut self, no_from_top: usize) -> Result<(), ExitError> {
		if self.data.len() <= no_from_top {
			return Err(ExitError::StackUnderflow);
		}

		let index = self.data.len() - no_from_top - 1;
		self.push_u256(self.data[index])
	}

	/// Swap a value at given index with the top value
	pub fn swap(&mut self, no_from_top: usize) -> Result<(), ExitError> {
		if self.data.len() <= no_from_top {
			return Err(ExitError::StackUnderflow);
		}

		let len = self.data.len();
		let a = len - no_from_top - 1;
		let b = len - 1;

		self.data.swap(a, b);

		Ok(())
	}
}
