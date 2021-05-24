//! Core layer for EVM.

#![deny(warnings)]
#![forbid(unused_variables, unused_imports)]

#![cfg_attr(not(feature = "std"), no_std)]

extern crate core;
extern crate alloc;

mod code;
mod memory;
mod stack;
mod valids;
mod opcode;
mod error;
mod eval;
mod utils;

pub use crate::code::Code;
pub use crate::memory::Memory;
pub use crate::stack::Stack;
pub use crate::valids::Valids;
pub use crate::opcode::Opcode;
pub use crate::error::{Trap, Capture, ExitReason, ExitSucceed, ExitError, ExitRevert, ExitFatal};

use core::ops::Range;
use alloc::vec::Vec;
use crate::eval::{eval, Control};

/// Core execution layer for EVM.
#[cfg_attr(feature = "with-codec", derive(codec::Encode, codec::Decode))]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Machine {
	/// Program data.
	#[cfg_attr(feature = "with-serde", serde(with = "serde_bytes"))]
	data: Vec<u8>,
	/// Program code.
	code: Code,
	/// Program counter.
	position: Result<usize, ExitReason>,
	/// Return value.
	return_range: Range<usize>,
	/// Code validity maps.
	valids: Valids,
	/// Memory.
	memory: Memory,
	/// Stack.
	stack: Stack,
}

impl Machine {
	/// Reference of machine stack.
	pub fn stack(&self) -> &Stack { &self.stack }
	/// Mutable reference of machine stack.
	pub fn stack_mut(&mut self) -> &mut Stack { &mut self.stack }
	/// Reference of machine memory.
	pub fn memory(&self) -> &Memory { &self.memory }
	/// Mutable reference of machine memory.
	pub fn memory_mut(&mut self) -> &mut Memory { &mut self.memory }
	/// Reference of machine code.
	pub fn code(&self) -> &Code { &self.code }
	/// Mutable reference of machine code.
	pub fn code_mut(&mut self) -> &mut Code { &mut self.code }

	/// Create a new machine with given code and data.
	pub fn new(
		code: Code,
		data: Vec<u8>,
		stack_limit: usize,
		memory_limit: usize
	) -> Self {
		let valids = Valids::new(&code[..]);

		Self {
			data,
			code,
			position: Ok(0),
			return_range: 0..0,
			valids,
			memory: Memory::new(memory_limit),
			stack: Stack::new(stack_limit),
		}
	}

	/// Explict exit of the machine. Further step will return error.
	pub fn exit(&mut self, reason: ExitReason) {
		self.position = Err(reason);
	}

	/// Inspect the machine's next opcode and current stack.
	pub fn inspect(&self) -> Option<(Opcode, &Stack)> {
		let position = match self.position {
			Ok(position) => position,
			Err(_) => return None,
		};
		self.code.get(position).map(|v| (Opcode(*v), &self.stack))
	}

	/// Copy and get the return value of the machine, if any.
	pub fn return_value(&self) -> Vec<u8> {
		self.memory.get(
			self.return_range.start,
			self.return_range.end - self.return_range.start,
		)
	}

	/// Loop stepping the machine, until it stops.
	pub fn run(&mut self) -> Capture<ExitReason, Trap> {
		loop {
			match self.step() {
				Ok(()) => (),
				Err(res) => return res,
			}
		}
	}

	/// Step the machine, executing one opcode. It then returns.
	pub fn step(&mut self) -> Result<(), Capture<ExitReason, Trap>> {
		let position = *self.position.as_ref().map_err(|reason| Capture::Exit(reason.clone()))?;

		match self.code.get(position).map(|v| Opcode(*v)) {
			Some(opcode) => {
				match eval(self, opcode, position) {
					Control::Continue(p) => {
						self.position = Ok(position + p);
						Ok(())
					},
					Control::Exit(e) => {
						self.position = Err(e.clone());
						Err(Capture::Exit(e))
					},
					Control::Jump(p) => {
						self.position = Ok(p);
						Ok(())
					},
					Control::Trap(opcode) => {
						self.position = Ok(position + 1);
						Err(Capture::Trap(opcode))
					},
				}
			},
			None => {
				self.position = Err(ExitSucceed::Stopped.into());
				Err(Capture::Exit(ExitSucceed::Stopped.into()))
			},
		}
	}
}
