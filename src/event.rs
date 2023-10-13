use async_trait::async_trait;

use crate::effect::{Effect, Effects};

#[async_trait]
pub trait Event<T>: Send + Sync + 'static {
    fn to_effect(self) -> Effect<T>
    where
        Self: Sized + 'static,
    {
        Effect::Event(Box::new(self))
    }

    async fn handle(&self, state: &T) -> Effects<T>;
}
