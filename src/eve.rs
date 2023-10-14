use std::sync::Arc;

use async_trait::async_trait;
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
};

use crate::effect::{Action, Event, Events, Query, Transaction};

#[derive(Clone)]
pub struct Eve<T> {
    router: Router<T>,
}

impl<T: Send + Sync + 'static> Eve<T> {
    pub fn new(global_state: T) -> Self {
        let (action_handler, query_handler, transaction_handler, router) =
            create_eve_handlers(Arc::new(RwLock::new(global_state)));
        tokio::spawn(run_handler_loop(action_handler));
        tokio::spawn(run_handler_loop(query_handler));
        tokio::spawn(run_handler_loop(transaction_handler));
        Self { router }
    }
    pub async fn dispatch(&self, event: impl Into<Event<T>>) {
        self.router.relay(event.into()).await;
    }
}

pub struct EveActionHandler<T> {
    pub global_state: Arc<RwLock<T>>,
    pub action_rx: mpsc::Receiver<Box<dyn Action<T>>>,
    pub router: Router<T>,
}

impl<T> EveActionHandler<T> {
    pub fn new(
        global_state: Arc<RwLock<T>>,
        action_rx: mpsc::Receiver<Box<dyn Action<T>>>,
        router: Router<T>,
    ) -> Self {
        Self {
            global_state,
            action_rx,
            router,
        }
    }
}

pub struct EveQueryHandler<T> {
    pub global_state: Arc<RwLock<T>>,
    pub query_rx: mpsc::Receiver<Box<dyn Query<T>>>,
    pub router: Router<T>,
}

impl<T> EveQueryHandler<T> {
    pub fn new(
        global_state: Arc<RwLock<T>>,
        query_rx: mpsc::Receiver<Box<dyn Query<T>>>,
        router: Router<T>,
    ) -> Self {
        Self {
            global_state,
            query_rx,
            router,
        }
    }
}

pub struct EveTransactionHandler<T> {
    pub global_state: Arc<RwLock<T>>,
    pub transaction_rx: mpsc::Receiver<Box<dyn Transaction<T>>>,
    pub router: Router<T>,
}

impl<T> EveTransactionHandler<T> {
    pub fn new(
        global_state: Arc<RwLock<T>>,
        transaction_rx: mpsc::Receiver<Box<dyn Transaction<T>>>,
        router: Router<T>,
    ) -> Self {
        Self {
            global_state,
            transaction_rx,
            router,
        }
    }
}

pub struct Router<T> {
    pub action_tx: mpsc::Sender<Box<dyn Action<T>>>,
    pub query_tx: mpsc::Sender<Box<dyn Query<T>>>,
    pub transaction_tx: mpsc::Sender<Box<dyn Transaction<T>>>,
}

impl<T> Clone for Router<T> {
    fn clone(&self) -> Self {
        Self {
            action_tx: self.action_tx.clone(),
            query_tx: self.query_tx.clone(),
            transaction_tx: self.transaction_tx.clone(),
        }
    }
}

pub fn create_eve_handlers<T: Send + Sync + 'static>(
    global_state: Arc<RwLock<T>>,
) -> (
    EveActionHandler<T>,
    EveQueryHandler<T>,
    EveTransactionHandler<T>,
    Router<T>,
) {
    let (action_tx, action_rx) = mpsc::channel(100);
    let (query_tx, query_rx) = mpsc::channel(100);
    let (transaction_tx, transaction_rx) = mpsc::channel(100);
    let router = Router {
        action_tx,
        query_tx,
        transaction_tx,
    };
    (
        EveActionHandler::new(global_state.clone(), action_rx, router.clone()),
        EveQueryHandler::new(global_state.clone(), query_rx, router.clone()),
        EveTransactionHandler::new(global_state, transaction_rx, router.clone()),
        router,
    )
}

impl<T> Router<T> {
    pub async fn relay(&self, event: Event<T>) {
        match event {
            Event::Action(event) => {
                self.action_tx
                    .send(event)
                    .await
                    .expect("Action channel closed");
            }
            Event::Query(event) => {
                self.query_tx
                    .send(event)
                    .await
                    .expect("Query channel closed");
            }
            Event::Transaction(event) => {
                self.transaction_tx
                    .send(event)
                    .await
                    .expect("Transaction channel closed");
            }
        }
    }
}

#[async_trait]
pub trait Handler<T: Send + Sync + 'static> {
    type EventType: Send + Sync;

    async fn next_event(&mut self) -> Self::EventType;
    async fn handle(&self, event: Self::EventType);
}

pub async fn run_handler_loop<T, H>(mut handler: H) -> JoinHandle<()>
where
    T: 'static + Send + Sync,
    H: Handler<T> + Send + 'static,
{
    loop {
        let event = handler.next_event().await;
        handler.handle(event).await;
    }
}

#[async_trait]
impl<T: 'static + Send + Sync> Handler<T> for EveActionHandler<T> {
    type EventType = Box<dyn Action<T>>;
    async fn next_event(&mut self) -> Self::EventType {
        self.action_rx.recv().await.expect("Action channel closed")
    }

    async fn handle(self, event: Self::EventType) {
        tokio::spawn(async move {
            event.handle(&*self.global_state.clone().read().await).await;
        });
        // event
        //     .execute(&*self.global_state.clone().read().await, &self.router)
        //     .await;
    }
}

#[async_trait]
impl<T: 'static + Send + Sync> Handler<T> for EveQueryHandler<T> {
    type EventType = Box<dyn Query<T>>;
    async fn next_event(&mut self) -> Self::EventType {
        self.query_rx.recv().await.expect("Query channel closed")
    }
    async fn handle(&self, event: Self::EventType) {
        event
            .execute(&*self.global_state.clone().read().await, &self.router)
            .await;
    }
}

#[async_trait]
impl<T: 'static + Send + Sync> Handler<T> for EveTransactionHandler<T> {
    type EventType = Box<dyn Transaction<T>>;
    async fn next_event(&mut self) -> Self::EventType {
        self.transaction_rx
            .recv()
            .await
            .expect("Transaction channel closed")
    }
    async fn handle(&self, event: Self::EventType) {
        event
            .execute(&mut *self.global_state.clone().write().await, &self.router)
            .await;
    }
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

        impl From<TestEvent> for Event<GlobalState> {
            fn from(event: TestEvent) -> Self {
                Event::Action(Box::new(event))
            }
        }

        #[async_trait]
        impl Action<GlobalState> for TestEvent {
            async fn handle(&self, state: &GlobalState) -> Option<Events<GlobalState>> {
                println!("TestEvent::handle {:?}", state);
                Some(Events::new().push(TestEvent {}))
            }
        }

        // #[async_trait]
        // impl Transaction<GlobalState> for TestEvent {
        //     async fn handle(&self, state: &mut GlobalState) -> Events<GlobalState> {
        //         state.a += 1;
        //         println!("TestEvent::handle {:?}", state);
        //         Events::new().add_action(TestEvent {})
        //     }
        // }

        let eve = Eve::new(GlobalState::default());
        eve.dispatch(TestEvent {}).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
