//! EVM gasometer.

#![deny(warnings)]
#![forbid(unsafe_code, unused_variables)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
	clippy::module_name_repetitions,
	clippy::missing_errors_doc,
	clippy::missing_panics_doc
)]

#![cfg_attr(not(feature = "tracing"), forbid(unused_imports))]
#![cfg_attr(not(feature = "std"), no_std)]

mod consts;
mod costs;
mod memory;
mod utils;

use evm_core::{ExitError, Opcode, Stack, H160, H256, U256};
use evm_runtime::{CONFIG, Handler};

pub mod tracing;

macro_rules! try_or_fail {
	( $inner:expr, $e:expr ) => (
		match $e {
			Ok(value) => value,
			Err(e) => {
				$inner = Err(e.clone());
				return Err(e)
			},
		}
	)
}

/// EVM gasometer.
#[derive(Clone)]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Gasometer {
	gas_limit: u64,
	inner: Result<Inner, ExitError>
}

impl Gasometer {
	/// Create a new gasometer with given gas limit.
	pub fn new(gas_limit: u64) -> Self {
		Self {
			gas_limit,
			inner: Ok(Inner {
				memory_cost: 0,
				used_gas: 0,
				refunded_gas: 0,
			}),
		}
	}

	fn inner_mut(
		&mut self
	) -> Result<&mut Inner, ExitError> {
		self.inner.as_mut().map_err(|e| e.clone())
	}


	/// Remaining gas.
	pub fn gas(&self) -> u64 {
		match self.inner.as_ref() {
			Ok(inner) => {
				self.gas_limit - inner.used_gas -
					memory::memory_gas(inner.memory_cost).expect("Checked via record")
			},
			Err(_) => 0,
		}
	}

	/// Total used gas.
	pub fn total_used_gas(&self) -> u64 {
		match self.inner.as_ref() {
			Ok(inner) => inner.used_gas +
				memory::memory_gas(inner.memory_cost).expect("Checked via record"),
			Err(_) => self.gas_limit,
		}
	}

	/// Refunded gas.
	pub fn refunded_gas(&self) -> i64 {
		match self.inner.as_ref() {
			Ok(inner) => inner.refunded_gas,
			Err(_) => 0,
		}
	}

	/// Used gas.
	pub fn used_gas(&self) -> u64 {
		match self.inner.as_ref() {
			Ok(inner) => {
				let mg = memory::memory_gas(inner.memory_cost).expect("Checked via record");
				let tug = inner.used_gas + mg;
				let rg = inner.refunded_gas;
				tug - core::cmp::min(tug / 2, rg as u64)
			},
			Err(_) => 0,
		}
	}

	/// Explicitly fail the gasometer with out of gas. Return `OutOfGas` error.
	pub fn fail(&mut self) -> ExitError {
		self.inner = Err(ExitError::OutOfGas);
		ExitError::OutOfGas
	}

	/// Record an explicit cost.
	pub fn record_cost(
		&mut self,
		cost: u64
	) -> Result<(), ExitError> {
		tracing::Event::RecordCost(cost).emit();

		let all_gas_cost = self.total_used_gas() + cost;
		if self.gas_limit < all_gas_cost {
			self.inner = Err(ExitError::OutOfGas);
			return Err(ExitError::OutOfGas)
		}

		self.inner_mut()?.used_gas += cost;
		Ok(())
	}

	/// Record an explicit refund.
	pub fn record_refund(
		&mut self,
		refund: i64,
	) -> Result<(), ExitError> {
		tracing::Event::RecordRefund(refund).emit();

		self.inner_mut()?.refunded_gas += refund;
		Ok(())
	}

	/// Record `CREATE` code deposit.
	pub fn record_deposit(
		&mut self,
		len: usize
	) -> Result<(), ExitError> {
		let cost = len as u64 * consts::G_CODEDEPOSIT;
		self.record_cost(cost)
	}

	/// Record opcode gas cost.
	pub fn record_dynamic_cost(
		&mut self,
		cost: GasCost,
		memory: Option<MemoryCost>,
	) -> Result<(), ExitError> {
		let gas = self.gas();

		let memory_cost = match memory {
			Some(memory) => try_or_fail!(self.inner, self.inner_mut()?.memory_cost(memory)),
			None => self.inner_mut()?.memory_cost,
		};
		let memory_gas = try_or_fail!(self.inner, memory::memory_gas(memory_cost));
		let gas_cost = try_or_fail!(self.inner, self.inner_mut()?.gas_cost(cost.clone(), gas));
		let gas_refund = self.inner_mut()?.gas_refund(cost.clone());
		let used_gas = self.inner_mut()?.used_gas;

		tracing::Event::RecordDynamicCost {gas_cost, memory_gas, gas_refund}.emit();

		let all_gas_cost = memory_gas + used_gas + gas_cost;
		if self.gas_limit < all_gas_cost {
			self.inner = Err(ExitError::OutOfGas);
			return Err(ExitError::OutOfGas)
		}

		let after_gas = self.gas_limit - all_gas_cost;
		try_or_fail!(self.inner, self.inner_mut()?.extra_check(cost, after_gas));

		self.inner_mut()?.used_gas += gas_cost;
		self.inner_mut()?.memory_cost = memory_cost;
		self.inner_mut()?.refunded_gas += gas_refund;

		Ok(())
	}

	/// Record opcode stipend.
	pub fn record_stipend(
		&mut self,
		stipend: u64,
	) -> Result<(), ExitError> {
		tracing::Event::RecordStipend(stipend).emit();

		self.inner_mut()?.used_gas -= stipend;
		Ok(())
	}

	/// Record transaction cost.
	pub fn record_transaction(
		&mut self,
		cost: TransactionCost,
	) -> Result<(), ExitError> {
		let gas_cost = match cost {
			TransactionCost::Call { zero_data_len, non_zero_data_len } => {
				CONFIG.gas_transaction_call +
					zero_data_len as u64 * CONFIG.gas_transaction_zero_data +
					non_zero_data_len as u64  * CONFIG.gas_transaction_non_zero_data
			},
			TransactionCost::Create { zero_data_len, non_zero_data_len } => {
				CONFIG.gas_transaction_create +
					zero_data_len as u64 * CONFIG.gas_transaction_zero_data +
					non_zero_data_len as u64 * CONFIG.gas_transaction_non_zero_data
			},
		};

		tracing::Event::RecordTransaction(gas_cost).emit();

		if self.gas() < gas_cost {
			self.inner = Err(ExitError::OutOfGas);
			return Err(ExitError::OutOfGas);
		}

		self.inner_mut()?.used_gas += gas_cost;
		Ok(())
	}
}

/// Calculate the call transaction cost.
pub fn call_transaction_cost(
	data: &[u8]
) -> TransactionCost {
	let zero_data_len = data.iter().filter(|v| **v == 0).count();
	let non_zero_data_len = data.len() - zero_data_len;

	TransactionCost::Call { zero_data_len, non_zero_data_len }
}

/// Calculate the create transaction cost.
pub fn create_transaction_cost(
	data: &[u8]
) -> TransactionCost {
	let zero_data_len = data.iter().filter(|v| **v == 0).count();
	let non_zero_data_len = data.len() - zero_data_len;

	TransactionCost::Create { zero_data_len, non_zero_data_len }
}

pub fn static_opcode_cost(
	opcode: Opcode,
) -> Option<u64> {
	static TABLE: [Option<u64>; 256] = {
		let mut table: [Option<u64>; 256] = [None; 256];

		table[Opcode::STOP.as_usize()] = Some(consts::G_ZERO);
		table[Opcode::CALLDATASIZE.as_usize()] = Some(consts::G_BASE);
		table[Opcode::CODESIZE.as_usize()] = Some(consts::G_BASE);
		table[Opcode::POP.as_usize()] = Some(consts::G_BASE);
		table[Opcode::PC.as_usize()] = Some(consts::G_BASE);
		table[Opcode::MSIZE.as_usize()] = Some(consts::G_BASE);

		table[Opcode::ADDRESS.as_usize()] = Some(consts::G_BASE);
		table[Opcode::ORIGIN.as_usize()] = Some(consts::G_BASE);
		table[Opcode::CALLER.as_usize()] = Some(consts::G_BASE);
		table[Opcode::CALLVALUE.as_usize()] = Some(consts::G_BASE);
		table[Opcode::COINBASE.as_usize()] = Some(consts::G_BASE);
		table[Opcode::TIMESTAMP.as_usize()] = Some(consts::G_BASE);
		table[Opcode::NUMBER.as_usize()] = Some(consts::G_BASE);
		table[Opcode::DIFFICULTY.as_usize()] = Some(consts::G_BASE);
		table[Opcode::GASLIMIT.as_usize()] = Some(consts::G_BASE);
		table[Opcode::GASPRICE.as_usize()] = Some(consts::G_BASE);
		table[Opcode::GAS.as_usize()] = Some(consts::G_BASE);

		table[Opcode::ADD.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SUB.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::NOT.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::LT.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::GT.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SLT.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SGT.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::EQ.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::ISZERO.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::AND.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::OR.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::XOR.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::BYTE.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::CALLDATALOAD.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH1.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH2.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH3.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH4.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH5.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH6.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH7.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH8.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH9.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH10.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH11.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH12.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH13.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH14.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH15.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH16.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH17.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH18.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH19.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH20.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH21.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH22.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH23.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH24.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH25.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH26.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH27.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH28.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH29.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH30.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH31.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::PUSH32.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP1.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP2.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP3.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP4.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP5.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP6.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP7.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP8.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP9.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP10.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP11.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP12.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP13.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP14.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP15.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::DUP16.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP1.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP2.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP3.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP4.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP5.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP6.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP7.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP8.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP9.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP10.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP11.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP12.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP13.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP14.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP15.as_usize()] = Some(consts::G_VERYLOW);
		table[Opcode::SWAP16.as_usize()] = Some(consts::G_VERYLOW);

		table[Opcode::MUL.as_usize()] = Some(consts::G_LOW);
		table[Opcode::DIV.as_usize()] = Some(consts::G_LOW);
		table[Opcode::SDIV.as_usize()] = Some(consts::G_LOW);
		table[Opcode::MOD.as_usize()] = Some(consts::G_LOW);
		table[Opcode::SMOD.as_usize()] = Some(consts::G_LOW);
		table[Opcode::SIGNEXTEND.as_usize()] = Some(consts::G_LOW);

		table[Opcode::ADDMOD.as_usize()] = Some(consts::G_MID);
		table[Opcode::MULMOD.as_usize()] = Some(consts::G_MID);
		table[Opcode::JUMP.as_usize()] = Some(consts::G_MID);

		table[Opcode::JUMPI.as_usize()] = Some(consts::G_HIGH);
		table[Opcode::JUMPDEST.as_usize()] = Some(consts::G_JUMPDEST);

		table
	};

	TABLE[opcode.as_usize()]
}

/// Calculate the opcode cost.
pub fn dynamic_opcode_cost<H: Handler>(
	address: H160,
	opcode: Opcode,
	stack: &Stack,
	is_static: bool,
	handler: &H
) -> Result<(GasCost, Option<MemoryCost>), ExitError> {
	let gas_cost = match opcode {
		Opcode::RETURN => GasCost::Zero,

		Opcode::MLOAD | Opcode::MSTORE | Opcode::MSTORE8 => GasCost::VeryLow,

		Opcode::REVERT if CONFIG.has_revert => GasCost::Zero,
		Opcode::REVERT => GasCost::Invalid,

		Opcode::CHAINID if CONFIG.has_chain_id => GasCost::Base,
		Opcode::CHAINID => GasCost::Invalid,

		Opcode::SHL | Opcode::SHR | Opcode::SAR if CONFIG.has_bitwise_shifting =>
			GasCost::VeryLow,
		Opcode::SHL | Opcode::SHR | Opcode::SAR => GasCost::Invalid,

		Opcode::SELFBALANCE if CONFIG.has_self_balance => GasCost::Low,
		Opcode::SELFBALANCE => GasCost::Invalid,

		Opcode::EXTCODESIZE => GasCost::ExtCodeSize,
		Opcode::BALANCE => GasCost::Balance,
		Opcode::BLOCKHASH => GasCost::BlockHash,

		Opcode::EXTCODEHASH if CONFIG.has_ext_code_hash => GasCost::ExtCodeHash,
		Opcode::EXTCODEHASH => GasCost::Invalid,

		Opcode::CALLCODE => GasCost::CallCode {
			value: stack.peek(2)?,
			gas: stack.peek(0)?,
			target_exists: handler.exists(stack.peek(1)?.into()),
		},
		Opcode::STATICCALL => GasCost::StaticCall {
			gas: stack.peek(0)?,
			target_exists: handler.exists(stack.peek(1)?.into()),
		},
		Opcode::SHA3 => GasCost::Sha3 {
			len: stack.peek(1)?,
		},
		Opcode::EXTCODECOPY => GasCost::ExtCodeCopy {
			len: stack.peek(3)?,
		},
		Opcode::CALLDATACOPY | Opcode::CODECOPY => GasCost::VeryLowCopy {
			len: stack.peek(2)?,
		},
		Opcode::EXP => GasCost::Exp {
			power: stack.peek(1)?,
		},
		Opcode::SLOAD => GasCost::SLoad,

		Opcode::DELEGATECALL if CONFIG.has_delegate_call => GasCost::DelegateCall {
			gas: stack.peek(0)?,
			target_exists: handler.exists(stack.peek(1)?.into()),
		},
		Opcode::DELEGATECALL => GasCost::Invalid,

		Opcode::RETURNDATASIZE if CONFIG.has_return_data => GasCost::Base,
		Opcode::RETURNDATACOPY if CONFIG.has_return_data => GasCost::VeryLowCopy {
			len: stack.peek(2)?,
		},
		Opcode::RETURNDATASIZE | Opcode::RETURNDATACOPY => GasCost::Invalid,

		Opcode::SSTORE if !is_static => {
			let index = stack.peek(0)?;
			let value = stack.peek(1)?;

			GasCost::SStore {
				original: handler.original_storage(address, index).into(),
				current: handler.storage(address, index).into(),
				new: value.into(),
			}
		},
		Opcode::LOG0 if !is_static => GasCost::Log {
			n: 0,
			len: stack.peek(1)?,
		},
		Opcode::LOG1 if !is_static => GasCost::Log {
			n: 1,
			len: stack.peek(1)?,
		},
		Opcode::LOG2 if !is_static => GasCost::Log {
			n: 2,
			len: stack.peek(1)?,
		},
		Opcode::LOG3 if !is_static => GasCost::Log {
			n: 3,
			len: stack.peek(1)?,
		},
		Opcode::LOG4 if !is_static => GasCost::Log {
			n: 4,
			len: stack.peek(1)?,
		},
		Opcode::CREATE if !is_static => GasCost::Create,
		Opcode::CREATE2 if !is_static && CONFIG.has_create2 => GasCost::Create2 {
			len: stack.peek(2)?,
		},
		Opcode::SUICIDE if !is_static => GasCost::Suicide {
			value: handler.balance(address),
			target_exists: handler.exists(stack.peek(0)?.into()),
			already_removed: handler.deleted(address),
		},
		Opcode::CALL
			if !is_static ||
			(is_static && stack.peek(2)?.is_zero()) =>
			GasCost::Call {
				value: stack.peek(2)?,
				gas: stack.peek(0)?,
				target_exists: handler.exists(stack.peek(1)?.into()),
			},

		_ => GasCost::Invalid,
	};

	let memory_cost = match opcode {
		Opcode::SHA3 | Opcode::RETURN | Opcode::REVERT |
		Opcode::LOG0 | Opcode::LOG1 | Opcode::LOG2 |
		Opcode::LOG3 | Opcode::LOG4 => Some(MemoryCost {
			offset: stack.peek(0)?,
			len: stack.peek(1)?,
		}),

		Opcode::CODECOPY | Opcode::CALLDATACOPY |
		Opcode::RETURNDATACOPY => Some(MemoryCost {
			offset: stack.peek(0)?,
			len: stack.peek(2)?,
		}),

		Opcode::EXTCODECOPY => Some(MemoryCost {
			offset: stack.peek(1)?,
			len: stack.peek(3)?,
		}),

		Opcode::MLOAD | Opcode::MSTORE => Some(MemoryCost {
			offset: stack.peek(0)?,
			len: U256::from(32),
		}),

		Opcode::MSTORE8 => Some(MemoryCost {
			offset: stack.peek(0)?,
			len: U256::from(1),
		}),

		Opcode::CREATE | Opcode::CREATE2 => Some(MemoryCost {
			offset: stack.peek(1)?,
			len: stack.peek(2)?,
		}),

		Opcode::CALL | Opcode::CALLCODE => Some(MemoryCost {
			offset: stack.peek(3)?,
			len: stack.peek(4)?,
		}.join(MemoryCost {
			offset: stack.peek(5)?,
			len: stack.peek(6)?,
		})),

		Opcode::DELEGATECALL |
		Opcode::STATICCALL => Some(MemoryCost {
			offset: stack.peek(2)?,
			len: stack.peek(3)?,
		}.join(MemoryCost {
			offset: stack.peek(4)?,
			len: stack.peek(5)?,
		})),

		_ => None,
	};

	Ok((gas_cost, memory_cost))
}

#[derive(Clone)]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
struct Inner {
	memory_cost: u64,
	used_gas: u64,
	refunded_gas: i64,
}

impl Inner {
	fn memory_cost(
		&self,
		memory: MemoryCost,
	) -> Result<u64, ExitError> {
		let from = memory.offset;
		let len = memory.len;

		if len.is_zero() {
			return Ok(self.memory_cost)
		}

		let end = from.checked_add(len).ok_or(ExitError::OutOfGas)?;

		if end > U256::from(usize::max_value()) {
			return Err(ExitError::OutOfGas)
		}
		let end = end.as_usize();

		let rem = end % 32;
		let new = if rem == 0 {
			end / 32
		} else {
			end / 32 + 1
		};

		Ok(core::cmp::max(self.memory_cost, new as u64))
	}

	fn extra_check(
		&self,
		cost: GasCost,
		after_gas: u64,
	) -> Result<(), ExitError> {
		match cost {
			GasCost::Call { gas, .. } => costs::call_extra_check(gas, after_gas),
			GasCost::CallCode { gas, .. } => costs::call_extra_check(gas, after_gas),
			GasCost::DelegateCall { gas, .. } => costs::call_extra_check(gas, after_gas),
			GasCost::StaticCall { gas, .. } => costs::call_extra_check(gas, after_gas),
			_ => Ok(()),
		}
	}

	/// Returns the gas cost numerical value.
	fn gas_cost(
		&self,
		cost: GasCost,
		gas: u64,
	) -> Result<u64, ExitError> {
		Ok(match cost {
			GasCost::Call { value, target_exists, .. } =>
				costs::call_cost(value, true, true, !target_exists),
			GasCost::CallCode { value, target_exists, .. } =>
				costs::call_cost(value, true, false, !target_exists),
			GasCost::DelegateCall { target_exists, .. } =>
				costs::call_cost(U256::zero(), false, false, !target_exists),
			GasCost::StaticCall { target_exists, .. } =>
				costs::call_cost(U256::zero(), false, true, !target_exists),
			GasCost::Suicide { value, target_exists, .. } =>
				costs::suicide_cost(value, target_exists),
			GasCost::SStore { .. } if CONFIG.estimate => CONFIG.gas_sstore_set,
			GasCost::SStore { original, current, new } =>
				costs::sstore_cost(original, current, new, gas)?,

			GasCost::Sha3 { len } => costs::sha3_cost(len)?,
			GasCost::Log { n, len } => costs::log_cost(n, len)?,
			GasCost::ExtCodeCopy { len } => costs::extcodecopy_cost(len)?,
			GasCost::VeryLowCopy { len } => costs::verylowcopy_cost(len)?,
			GasCost::Exp { power } => costs::exp_cost(power)?,
			GasCost::Create => consts::G_CREATE,
			GasCost::Create2 { len } => costs::create2_cost(len)?,
			GasCost::SLoad => CONFIG.gas_sload,

			GasCost::Zero => consts::G_ZERO,
			GasCost::Base => consts::G_BASE,
			GasCost::VeryLow => consts::G_VERYLOW,
			GasCost::Low => consts::G_LOW,
			GasCost::Invalid => return Err(ExitError::OutOfGas),

			GasCost::ExtCodeSize => CONFIG.gas_ext_code,
			GasCost::Balance => CONFIG.gas_balance,
			GasCost::BlockHash => consts::G_BLOCKHASH,
			GasCost::ExtCodeHash => CONFIG.gas_ext_code_hash,
		})
	}

	fn gas_refund(
		&self,
		cost: GasCost
	) -> i64 {
		match cost {
			_ if CONFIG.estimate => 0,
			GasCost::SStore { original, current, new } =>
				costs::sstore_refund(original, current, new),
			GasCost::Suicide { already_removed, .. } =>
				costs::suicide_refund(already_removed),
			_ => 0,
		}
	}
}

/// Gas cost.
#[derive(Debug, Clone)]
pub enum GasCost {
	/// Zero gas cost.
	Zero,
	/// Base gas cost.
	Base,
	/// Very low gas cost.
	VeryLow,
	/// Low gas cost.
	Low,
	/// Fail the gasometer.
	Invalid,

	/// Gas cost for `EXTCODESIZE`.
	ExtCodeSize,
	/// Gas cost for `BALANCE`.
	Balance,
	/// Gas cost for `BLOCKHASH`.
	BlockHash,
	/// Gas cost for `EXTBLOCKHASH`.
	ExtCodeHash,

	/// Gas cost for `CALL`.
	Call {
		/// Call value.
		value: U256,
		/// Call gas.
		gas: U256,
		/// Whether the target exists.
		target_exists: bool
	},
	/// Gas cost for `CALLCODE.
	CallCode {
		/// Call value.
		value: U256,
		/// Call gas.
		gas: U256,
		/// Whether the target exists.
		target_exists: bool
	},
	/// Gas cost for `DELEGATECALL`.
	DelegateCall {
		/// Call gas.
		gas: U256,
		/// Whether the target exists.
		target_exists: bool
	},
	/// Gas cost for `STATICCALL`.
	StaticCall {
		/// Call gas.
		gas: U256,
		/// Whether the target exists.
		target_exists: bool
	},
	/// Gas cost for `SUICIDE`.
	Suicide {
		/// Value.
		value: U256,
		/// Whether the target exists.
		target_exists: bool,
		/// Whether the target has already been removed.
		already_removed: bool
	},
	/// Gas cost for `SSTORE`.
	SStore {
		/// Original value.
		original: H256,
		/// Current value.
		current: H256,
		/// New value.
		new: H256
	},
	/// Gas cost for `SHA3`.
	Sha3 {
		/// Length of the data.
		len: U256
	},
	/// Gas cost for `LOG`.
	Log {
		/// Topic length.
		n: u8,
		/// Data length.
		len: U256
	},
	/// Gas cost for `EXTCODECOPY`.
	ExtCodeCopy {
		/// Length.
		len: U256
	},
	/// Gas cost for some copy opcodes that is documented as `VERYLOW`.
	VeryLowCopy {
		/// Length.
		len: U256
	},
	/// Gas cost for `EXP`.
	Exp {
		/// Power of `EXP`.
		power: U256
	},
	/// Gas cost for `CREATE`.
	Create,
	/// Gas cost for `CREATE2`.
	Create2 {
		/// Length.
		len: U256
	},
	/// Gas cost for `SLOAD`.
	SLoad,
}

/// Memory cost.
#[derive(Debug, Clone)]
pub struct MemoryCost {
	/// Affected memory offset.
	pub offset: U256,
	/// Affected length.
	pub len: U256,
}

/// Transaction cost.
#[derive(Debug, Clone)]
pub enum TransactionCost {
	/// Call transaction cost.
	Call {
		/// Length of zeros in transaction data.
		zero_data_len: usize,
		/// Length of non-zeros in transaction data.
		non_zero_data_len: usize
	},
	/// Create transaction cost.
	Create {
		/// Length of zeros in transaction data.
		zero_data_len: usize,
		/// Length of non-zeros in transaction data.
		non_zero_data_len: usize
	},
}

impl MemoryCost {
	/// Join two memory cost together.
	pub fn join(self, other: MemoryCost) -> MemoryCost {
		if self.len.is_zero() {
			return other
		}

		if other.len.is_zero() {
			return self
		}

		let self_end = self.offset.saturating_add(self.len);
		let other_end = other.offset.saturating_add(other.len);

		if self_end >= other_end {
			self
		} else {
			other
		}
	}
}
