use std::{marker::PhantomData, sync::Arc};

use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
};

use crate::{
    effect::{Effect, Effects},
    event::Event,
    side_effect::{self, SideEffect},
};

pub struct EveEventHandler<T> {
    pub event_tx: mpsc::Sender<Box<dyn Event<T>>>,
    pub event_rx: mpsc::Receiver<Box<dyn Event<T>>>,
    pub side_effext_tx: mpsc::Sender<Box<dyn SideEffect<T>>>,
    pub global_state: Arc<RwLock<T>>,
}

pub struct EveSideEffectHandler<T> {
    pub event_tx: mpsc::Sender<Box<dyn Event<T>>>,
    pub side_effext_tx: mpsc::Sender<Box<dyn SideEffect<T>>>,
    pub side_effext_rx: mpsc::Receiver<Box<dyn SideEffect<T>>>,
    pub global_state: Arc<RwLock<T>>,
}

impl<T> EveEventHandler<T>
where
    T: Send + Sync + 'static,
{
    async fn next_event(&mut self) -> Box<dyn Event<T>> {
        self.event_rx.recv().await.expect("Event channel closed")
    }

    async fn handle_event(&self, event: Box<dyn Event<T>>) -> Effects<T> {
        event.handle(&*self.global_state.clone().read().await).await
    }

    async fn handle_effect(&self, effect: Effect<T>) {
        match effect {
            crate::effect::Effect::Event(event) => {
                self.event_tx
                    .send(event)
                    .await
                    .expect("Event channel closed");
            }
            crate::effect::Effect::SideEffects(side_effect) => {
                self.side_effext_tx
                    .send(side_effect)
                    .await
                    .expect("Side effect channel closed");
            }
        }
    }

    pub async fn spawn_loop(mut self) {
        tokio::spawn(async move {
            loop {
                let event = self.next_event().await;
                let effects = self.handle_event(event).await;
                for effect in effects.effects {
                    self.handle_effect(effect).await;
                }
            }
        });
    }
}

impl<T> EveSideEffectHandler<T>
where
    T: Send + Sync + 'static,
{
    async fn next_side_effect(&mut self) -> Box<dyn SideEffect<T>> {
        self.side_effext_rx
            .recv()
            .await
            .expect("Side effects channel closed")
    }

    async fn handle_side_effect(&self, side_effect: Box<dyn SideEffect<T>>) -> Effects<T> {
        side_effect
            .handle(&mut *self.global_state.clone().write().await)
            .await
    }
    pub async fn spawn_loop(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let side_effect = self.next_side_effect().await;
                self.handle_side_effect(side_effect).await;
            }
        })
    }
}

pub fn create_eve_handlers<T>(state: T) -> (EveEventHandler<T>, EveSideEffectHandler<T>) {
    let global_state = Arc::new(RwLock::new(state));
    let (event_tx, event_rx) = mpsc::channel(1024);
    let (side_effext_tx, side_effext_rx) = mpsc::channel(1024);
    let eve_event_handler = EveEventHandler {
        event_tx: event_tx.clone(),
        event_rx,
        side_effext_tx: side_effext_tx.clone(),
        global_state: global_state.clone(),
    };
    let eve_side_effect_handler = EveSideEffectHandler {
        event_tx,
        side_effext_tx,
        side_effext_rx,
        global_state,
    };
    (eve_event_handler, eve_side_effect_handler)
}

#[allow(dead_code)]
#[cfg(test)]
mod tests {

    use async_trait::async_trait;

    use super::*;

    #[tokio::test]
    async fn it_works() {
        #[derive(Debug, Default)]
        struct GlobalState {
            a: i32,
        }

        struct TestEvent {}

        #[async_trait]
        impl Event<GlobalState> for TestEvent {
            async fn handle(&self, state: &GlobalState) -> Effects<GlobalState> {
                println!("TestEvent::handle {:?}", state);
                Effects::new().add_side_effect(TestEvent {})
            }
        }

        #[async_trait]
        impl SideEffect<GlobalState> for TestEvent {
            async fn handle(&self, state: &mut GlobalState) -> Effects<GlobalState> {
                state.a += 1;
                println!("TestEvent::handle {:?}", state);
                Effects::new().add_event(TestEvent {})
            }
        }

        let (eve_event_handler, eve_side_effect_handler) =
            create_eve_handlers(GlobalState::default());
        let event_tx = eve_event_handler.event_tx.clone();
        eve_event_handler.spawn_loop().await;
        eve_side_effect_handler.spawn_loop().await;
        event_tx.send(Box::new(TestEvent {})).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
