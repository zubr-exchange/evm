//! Allows to listen to gasometer events.

#[cfg(feature = "tracing")]
environmental::environmental!(listener: dyn EventListener + 'static);

#[cfg(feature = "tracing")]
pub trait EventListener {
    fn event(
        &mut self,
        event: Event
    );
}

#[derive(Debug, Copy, Clone)]
pub struct Snapshot {
    pub gas_limit: u64,
    pub memory_gas: u64,
	pub used_gas: u64,
	pub refunded_gas: i64,
}

impl Snapshot {
    pub fn gas(&self) -> u64 {
        self.gas_limit - self.used_gas - self.memory_gas
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Event {
    RecordCost {
        cost: u64,
        snapshot: Snapshot,
    },
    RecordRefund {
        refund: i64,
        snapshot: Snapshot,
    },
    RecordStipend {
        stipend: u64,
        snapshot: Snapshot,
    },
    RecordDynamicCost {
        gas_cost: u64,
        memory_gas: u64,
        gas_refund: i64,
        snapshot: Snapshot,
    },
    RecordTransaction {
        cost: u64,
        snapshot: Snapshot,
    },
}

impl Event {
    #[cfg(feature = "tracing")]
    pub(crate) fn emit(self) {
        listener::with(|listener| listener.event(self));
    }

    #[cfg(not(feature = "tracing"))]
    pub(crate) fn emit(self) {
        // no op.
    }
}

/// Run closure with provided listener.
#[cfg(feature = "tracing")]
pub fn using<R, F: FnOnce() -> R>(
    new: &mut (dyn EventListener + 'static),
    f: F
) -> R {
    listener::using(new, f)
}