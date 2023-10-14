use async_trait::async_trait;

pub enum ExecutionType {
    Sequential,
    Concurrent,
    Parallel,
}

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
