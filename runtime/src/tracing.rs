//! Allows to listen to runtime events.

use crate::{Context, Opcode, Stack, Memory};
use crate::{H160, U256};



#[cfg(feature = "tracing")]
environmental::environmental!(hook: dyn EventListener + 'static);

#[cfg(feature = "tracing")]
pub trait EventListener {
    fn event(
        &mut self,
        event: Event
    );
}

#[derive(Debug, Copy, Clone)]
pub enum Event<'a> {
    Step {
        context: &'a Context,
        opcode: Opcode,
        stack: &'a Stack,
        memory: &'a Memory
    },
    SLoad {
        address: H160,
        index: U256,
        value: U256
    },
    SStore {
        address: H160,
        index: U256,
        value: U256
    },
}

impl<'a> Event<'a> {
    #[cfg(feature = "tracing")]
    pub(crate) fn emit(self) {
        hook::with(|hook| hook.event(self));
    }

    #[cfg(not(feature = "tracing"))]
    pub(crate) fn emit(self) {
        // no op.
    }
}

/// Run closure with provided listener.
#[cfg(feature = "tracing")]
pub fn using<R, F: FnOnce() -> R>(
    listener: &mut (dyn EventListener + 'static),
    f: F
) -> R {
    hook::using(listener, f)
}
