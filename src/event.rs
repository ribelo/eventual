use std::{ops::Deref, sync::Arc};

use async_trait::async_trait;
use downcast_rs::{impl_downcast, DowncastSync};
use tokio::sync::oneshot;

use crate::{id::Id, BoxableValue};

#[async_trait]
pub trait Eventable: DowncastSync + 'static {}
impl_downcast!(sync Eventable);

#[async_trait]
pub trait Respondable: DowncastSync + 'static {
    type Response: BoxableValue;
    async fn respond(&self, response: Self::Response);
}

pub struct Message {
    pub id: Id,
    pub event: Arc<dyn Eventable>,
    pub tx: Option<oneshot::Sender<()>>,
}

impl Message {
    pub fn new<E: Eventable>(event: Arc<E>) -> Self {
        Self {
            id: Id::new::<E>(),
            event,
            tx: None,
        }
    }
    pub fn new_sync<E: Eventable>(event: Arc<E>, tx: oneshot::Sender<()>) -> Self {
        Self {
            id: Id::new::<E>(),
            event,
            tx: Some(tx),
        }
    }
}

pub struct Event<E: Eventable>(pub Arc<E>);

impl<E: Eventable> Deref for Event<E> {
    type Target = Arc<E>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait IntoMessage {
    fn into_message(self, tx: Option<oneshot::Sender<()>>) -> Message;
}

impl<E> IntoMessage for Event<E>
where
    E: Eventable,
{
    fn into_message(self, tx: Option<oneshot::Sender<()>>) -> Message {
        match tx {
            Some(tx) => Message::new_sync::<E>(self.0, tx),
            None => Message::new::<E>(self.0),
        }
    }
}

impl<E> IntoMessage for Arc<E>
where
    E: Eventable,
{
    fn into_message(self, tx: Option<oneshot::Sender<()>>) -> Message {
        match tx {
            Some(tx) => Message::new_sync(self, tx),
            None => Message::new(self),
        }
    }
}

impl<E> IntoMessage for E
where
    E: Eventable,
{
    fn into_message(self, tx: Option<oneshot::Sender<()>>) -> Message {
        match tx {
            Some(tx) => Message::new_sync(Arc::new(self), tx),
            None => Message::new(Arc::new(self)),
        }
    }
}
