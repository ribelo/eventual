use std::{any::Any, fmt, marker::PhantomData};

use async_trait::async_trait;
use downcast_rs::DowncastSync;
use dyn_clone::DynClone;
use tokio::sync::oneshot;

use crate::{
    error::{ActionError, CommandError, QueryError},
    eve::Eventual,
    id::Id,
    magic_handler::{Context, FromContext},
};

#[async_trait]
pub trait Event: Send + Sync + DynClone + fmt::Debug + 'static {}
dyn_clone::clone_trait_object!(Event);

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
