use std::{any::Any, fmt, marker::PhantomData};

use async_trait::async_trait;
use downcast_rs::{impl_downcast, DowncastSync};
use dyn_clone::DynClone;
use tokio::sync::oneshot;

use crate::{
    eve::Eve,
    event_handler::{EventContext, FromEventContext},
    id::Id,
};

#[async_trait]
pub trait Eventable: DowncastSync + DynClone + fmt::Debug + 'static {}
impl_downcast!(sync Eventable);
dyn_clone::clone_trait_object!(Eventable);

#[derive(Debug)]
pub struct Message {
    pub id: Id,
    pub event: Box<dyn Eventable>,
    pub tx: Option<oneshot::Sender<()>>,
}

impl Message {
    pub fn new<E: Eventable>(event: E) -> Self {
        Self {
            id: Id::new::<E>(),
            event: Box::new(event),
            tx: None,
        }
    }
    pub fn new_sync<E: Eventable>(event: E, tx: oneshot::Sender<()>) -> Self {
        Self {
            id: Id::new::<E>(),
            event: Box::new(event),
            tx: Some(tx),
        }
    }
}
