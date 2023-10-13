use async_trait::async_trait;

use crate::effect::{Effect, Effects};

#[async_trait]
pub trait SideEffect<T>: Send + Sync + 'static {
    fn to_effect(self) -> Effect<T>
    where
        Self: Sized + 'static,
    {
        Effect::SideEffects(Box::new(self))
    }

    async fn handle(&self, state: &mut T) -> Effects<T>;
}
