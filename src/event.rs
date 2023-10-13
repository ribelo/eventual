use async_trait::async_trait;

use crate::{
    effect::{Effect, Effects},
    eve::Eve,
};

#[async_trait]
pub trait Event: Send + Sync + 'static {
    type Eve: Eve;

    fn to_effect(self) -> Effect<Self::Eve>
    where
        Self: Sized + 'static,
    {
        Effect::Event(Box::new(self))
    }

    async fn handle(&self, state: &<Self::Eve as Eve>::State) -> Effects<Self::Eve>;
}
