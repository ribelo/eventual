use std::any::Any;

use async_trait::async_trait;
use thiserror::Error;

pub enum ExecutionType {
    Sequential,
    Concurrent,
    Parallel,
}

pub struct CorrelationId(u64);

#[derive(Error, Debug)]
pub enum ResponderError {}

#[async_trait]
pub trait Responder {
    async fn respond(&self, data: Box<dyn Any + Send + Sync>) -> Result<(), ResponderError>;
}

pub enum Event<T> {
    Action {
        id: CorrelationId,
        event: Box<dyn Action<T>>,
        responder: Option<Box<dyn Responder>>,
    },
    Query {
        id: CorrelationId,
        event: Box<dyn Query<T>>,
        responder: Option<Box<dyn Responder>>,
    },
    Transaction {
        id: CorrelationId,
        event: Box<dyn Action<T>>,
        responder: Option<Box<dyn Responder>>,
    },
    Response {
        id: CorrelationId,
        event: Box<dyn Action<T>>,
        responder: Option<Box<dyn Responder>>,
    },
}

pub struct Events<T> {
    pub(crate) effects: Vec<Event<T>>,
}

#[async_trait]
pub trait Action<T: Send + Sync>: Send + Sync + 'static {
    fn execution_type(&self) -> ExecutionType {
        ExecutionType::Sequential
    }
    async fn handle(&self, state: &T) -> Events<T>;
}

#[async_trait]
pub trait Query<T: Send + Sync + 'static>: Send + Sync + 'static {
    fn execution_type(&self) -> ExecutionType {
        ExecutionType::Concurrent
    }
    async fn handle(&self, state: &T) -> Events<T>;
}

#[async_trait]
pub trait Transaction<T: Send + Sync + 'static>: Send + Sync + 'static {
    fn execution_type(&self) -> ExecutionType {
        ExecutionType::Sequential
    }
    async fn handle(&self, state: &mut T) -> Events<T>;
}

#[async_trait]
pub trait Response<T: Send + Sync + 'static>: Send + Sync + 'static {
    fn execution_type(&self) -> ExecutionType {
        ExecutionType::Sequential
    }
    async fn handle(&self) -> Events<T>;
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
