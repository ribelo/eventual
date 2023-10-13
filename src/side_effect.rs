use async_trait::async_trait;

use crate::{
    effect::{Effect, Effects},
    eve::Eve,
};

#[async_trait]
pub trait SideEffect: Send + Sync + 'static {
    type Eve: Eve;

    fn to_effect(self) -> Effect<Self::Eve>
    where
        Self: Sized + 'static,
    {
        Effect::SideEffects(Box::new(self))
    }

    async fn handle(&self, state: &mut <Self::Eve as Eve>::State) -> Effects<Self::Eve>;
}
