use std::sync::Arc;

use failure::_core::pin::Pin;
use futures::task::{Context, Poll};
use futures::{executor, Future, SinkExt, Stream};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use block_albatross::{MacroBlock, SignedViewChange, ViewChangeProof};
use blockchain_albatross::Blockchain;
use primitives::slot::ValidatorSlots;
use utils::observer::Notifier;

pub struct Tendermint;
impl Tendermint {
    pub fn new() -> Self {
        Self {}
    }
}
impl Future for Tendermint {
    type Output = Result<MacroBlock, ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unimplemented!()
    }
}

pub struct ViewChangeHandel;
impl ViewChangeHandel {
    pub fn new(
        signed_view_change: SignedViewChange,
        validator_id: u16,
        active_validator: ValidatorSlots,
    ) -> Self {
        unimplemented!()
    }
}
impl Future for ViewChangeHandel {
    type Output = ViewChangeProof;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unimplemented!()
    }
}

pub fn notifier_to_stream<E: Clone + Send + Sync + 'static>(
    notifier: &mut Notifier<E>,
) -> mpsc::UnboundedReceiver<E> {
    // TODO how to deregister?
    let (tx, rx) = mpsc::unbounded_channel();
    notifier.register(move |event: &E| {
        tx.send(event.clone());
    });
    rx
}