use async_trait::async_trait;

use crate::eve::{Handler, Router};

pub enum Event<T> {
    Action(Box<dyn Action<T>>),
    Query(Box<dyn Query<T>>),
    Transaction(Box<dyn Transaction<T>>),
}

pub struct Events<T> {
    pub(crate) effects: Vec<Event<T>>,
}

#[async_trait]
pub trait Action<T: Send + Sync>: Send + Sync + 'static {
    async fn execute(&self, state: &T, router: &Router<T>) {
        if let Some(events) = self.handle(state).await {
            for event in events.effects {
                router.relay(event).await;
            }
        }
    }
    async fn handle(&self, state: &T) -> Option<Events<T>>;
}

#[async_trait]
pub trait Query<T: Send + Sync + 'static>: Send + Sync + 'static {
    async fn execute(&self, state: &T, router: &Router<T>) {
        if let Some(events) = self.handle(state).await {
            for event in events.effects {
                router.relay(event).await;
            }
        }
    }
    async fn handle(&self, state: &T) -> Option<Events<T>>;
}

#[async_trait]
pub trait Transaction<T: Send + Sync + 'static>: Send + Sync + 'static {
    async fn route(&self, event: Event<T>, state: &T, router: Router<T>) {
        router.relay(event).await;
    }
    async fn execute(&self, state: &mut T, router: &Router<T>) {
        if let Some(events) = self.handle(state) {
            for event in events.effects {
                router.relay(event).await;
            }
        }
    }
    fn handle(&self, state: &mut T) -> Option<Events<T>>;
}

impl<T: Send + Sync> Default for Events<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync> Events<T> {
    pub fn new() -> Self {
        Self { effects: vec![] }
    }

    pub fn none() -> Self {
        Self::new()
    }

    pub fn push(mut self, effect: impl Into<Event<T>>) -> Self {
        self.effects.push(effect.into());
        self
    }

    pub fn extend(mut self, other: impl Into<Self>) -> Self {
        self.effects.extend(other.into().effects);
        self
    }
}

impl<T> From<Event<T>> for Events<T> {
    fn from(effect: Event<T>) -> Self {
        Self {
            effects: vec![effect],
        }
    }
}

impl<T> From<Box<dyn Action<T>>> for Events<T> {
    fn from(event: Box<dyn Action<T>>) -> Self {
        Self {
            effects: vec![Event::Action(event)],
        }
    }
}

impl<T> From<Box<dyn Query<T>>> for Event<T> {
    fn from(event: Box<dyn Query<T>>) -> Self {
        Self::Query(event)
    }
}

impl<T> From<Box<dyn Query<T>>> for Events<T> {
    fn from(event: Box<dyn Query<T>>) -> Self {
        Self {
            effects: vec![Event::Query(event)],
        }
    }
}

impl<T> From<Box<dyn Transaction<T>>> for Event<T> {
    fn from(event: Box<dyn Transaction<T>>) -> Self {
        Self::Transaction(event)
    }
}

impl<T> From<Box<dyn Transaction<T>>> for Events<T> {
    fn from(event: Box<dyn Transaction<T>>) -> Self {
        Self {
            effects: vec![Event::Transaction(event)],
        }
    }
}
