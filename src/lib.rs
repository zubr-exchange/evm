//! Ethereum Virtual Machine implementation in Rust

#![deny(warnings)]
#![forbid(unsafe_code, unused_variables)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
	clippy::module_name_repetitions,
	clippy::missing_errors_doc,
	clippy::missing_panics_doc
)]

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use evm_core::*;
pub use evm_runtime::*;
pub use evm_gasometer as gasometer;

#[cfg(feature = "tracing")]
pub mod tracing;

#[cfg(feature = "tracing")]
macro_rules! event {
	($x:expr) => {
		use crate::tracing::Event::*;
		crate::tracing::with(|listener| listener.event($x));
	};
}

#[cfg(not(feature = "tracing"))]
macro_rules! event {
	($x:expr) => {};
}

pub mod executor;
pub mod backend;
