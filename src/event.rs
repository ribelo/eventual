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
pub trait Event: DowncastSync + DynClone + fmt::Debug + 'static {}
impl_downcast!(sync Event);
dyn_clone::clone_trait_object!(Event);

#[derive(Debug)]
pub struct Message {
    pub id: Id,
    pub event: Box<dyn Event>,
}

impl Message {
    pub fn new<E: Event>(event: E) -> Self {
        Self {
            id: Id::new::<E>(),
            event: Box::new(event),
        }
    }
}
