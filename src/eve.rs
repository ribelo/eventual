use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    marker::PhantomData,
    mem,
    sync::{Arc, Mutex},
};
use tokio::sync::{
    mpsc::{self},
    oneshot,
};
use tracing::error;

use crate::{
    error::{DispatchError, NodeNotFoundError, RunEveError},
    event::{Eventable, Message},
    event_handler::{EventContext, EventHandler, EventHandlerFn, EventHandlerWrapper},
    id::Id,
    reactive::{NodeState, NodeValue, Reactive},
};

#[derive(Clone, Debug)]
pub struct Eve<S>
where
    S: Send + Sync,
{
    pub state: S,
    pub(crate) message_tx: mpsc::Sender<Message>,
    pub(crate) sync_tx: mpsc::Sender<Message>,
    pub(crate) handlers: Arc<HashMap<Id, Vec<Box<dyn EventHandlerFn<S>>>>>,

    //Reactively
    pub(crate) statuses: Arc<HashMap<Id, Arc<Mutex<NodeState>>>>,
    pub(crate) sources: Arc<HashMap<Id, Arc<HashSet<Id>>>>,
    pub(crate) subscribers: Arc<HashMap<Id, Arc<HashSet<Id>>>>,
    pub(crate) reactive: Arc<HashMap<Id, Arc<dyn Reactive<S>>>>,
    pub(crate) values: Arc<Mutex<HashMap<Id, NodeValue>>>,
    pub(crate) effects: Arc<HashSet<Id>>,
}

pub struct EveBuilder<S>
where
    S: Send + Sync,
{
    pub state: Option<S>,
    pub(crate) message_tx: mpsc::Sender<Message>,
    pub(crate) message_rx: Option<mpsc::Receiver<Message>>,
    pub(crate) sync_tx: mpsc::Sender<Message>,
    pub(crate) sync_rx: Option<mpsc::Receiver<Message>>,
    pub(crate) handlers: HashMap<Id, Vec<Box<dyn EventHandlerFn<S>>>>,
    pub(crate) statuses: HashMap<Id, NodeState>,
    pub(crate) sources: HashMap<Id, HashSet<Id>>,
    pub(crate) subscribers: HashMap<Id, HashSet<Id>>,
    pub(crate) reactive: HashMap<Id, Arc<dyn Reactive<S>>>,
    pub(crate) values: HashMap<Id, NodeValue>,
    pub(crate) effects: HashSet<Id>,
}

impl<S> EveBuilder<S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn new(state: S) -> Self {
        let (message_tx, message_rx) = mpsc::channel(16);
        let (sync_tx, sync_rx) = mpsc::channel(16);

        Self {
            state: Some(state),
            message_tx,
            message_rx: Some(message_rx),
            sync_tx,
            sync_rx: Some(sync_rx),
            handlers: Default::default(),
            statuses: Default::default(),
            sources: Default::default(),
            subscribers: Default::default(),
            reactive: Default::default(),
            values: Default::default(),
            effects: Default::default(),
        }
    }

    pub fn build(&mut self) -> Result<Eve<S>, RunEveError> {
        for deps in self.sources.values() {
            for dep_id in deps.iter() {
                if !self.statuses.contains_key(dep_id) {
                    return Err(NodeNotFoundError { id: *dep_id }.into());
                }
            }
        }

        let eve = Eve {
            state: self.state.take().unwrap(),
            message_tx: self.message_tx.clone(),
            sync_tx: self.sync_tx.clone(),
            handlers: Arc::new(mem::take(&mut self.handlers.clone())), // Clone before taking
            statuses: Arc::new(
                self.statuses
                    .drain()
                    .map(|(k, v)| (k, Arc::new(Mutex::new(v))))
                    .collect(),
            ), // Clone before taking
            sources: Arc::new(
                self.sources
                    .drain()
                    .map(|(k, v)| (k, Arc::new(v)))
                    .collect(),
            ),
            subscribers: Arc::new(
                self.subscribers
                    .drain()
                    .map(|(k, v)| (k, Arc::new(v)))
                    .collect(),
            ),
            reactive: Arc::new(mem::take(&mut self.reactive)), // Clone before taking
            values: Arc::new(Mutex::new(mem::take(&mut self.values))),
            effects: Arc::new(mem::take(&mut self.effects)), // Clone before taking
        };

        run_events_loop(
            self.message_rx.take().unwrap(),
            self.sync_rx.take().unwrap(),
            eve.clone(),
        );

        Ok(eve)
    }

    pub fn reg_handler<E, T, H>(mut self, handler: H) -> Result<Self, NodeNotFoundError>
    where
        E: Eventable + Clone,
        T: Send + Sync + 'static,
        H: EventHandler<E, S, T> + Copy + 'static,
    {
        // let mut deps = HashSet::default();
        // handler.collect_dependencies(&mut deps);

        let id = Id::new::<E>();
        let wrapper = EventHandlerWrapper {
            handler,
            phantom: PhantomData::<(E, S, T)>,
        };
        self.handlers.entry(id).or_default().push(Box::new(wrapper));

        Ok(self)
    }
}

impl<S> Eve<S>
where
    S: Send + Sync,
{
    pub fn get_handlers(&self, id: Id) -> Option<Vec<Box<dyn EventHandlerFn<S>>>> {
        self.handlers.get(&id).cloned()
    }

    pub async fn dispatch<E: Eventable>(&self, event: E) -> Result<(), DispatchError> {
        self.message_tx.send(Message::new(event)).await?;
        Ok(())
    }

    pub async fn dispatch_sync<E: Eventable>(&self, event: E) -> Result<(), DispatchError> {
        let (tx, rx) = oneshot::channel();
        let msg = Message::new_sync(event, tx);
        self.sync_tx.send(msg).await?;
        rx.await?;
        Ok(())
    }
}

fn run_events_loop<S>(
    mut message_rx: mpsc::Receiver<Message>,
    mut sync_rx: mpsc::Receiver<Message>,
    eve: Eve<S>,
) where
    S: Send + Sync + Clone + 'static,
{
    let cloned_eve = eve.clone();
    tokio::spawn(async move {
        loop {
            while let Some(msg) = message_rx.recv().await {
                if let Some(handlers) = cloned_eve.get_handlers(msg.id) {
                    let ctx = EventContext::new(msg.event, &cloned_eve);
                    for handler in handlers {
                        handler.call_with_context(&ctx).await;
                    }
                }
            }
        }
    });
    tokio::spawn(async move {
        loop {
            while let Some(msg) = sync_rx.recv().await {
                if let Some(handlers) = eve.get_handlers(msg.id) {
                    let ctx = EventContext::new(msg.event, &eve);
                    for handler in handlers {
                        handler.call_with_context(&ctx).await;
                    }
                    if let Some(tx) = msg.tx {
                        tx.send(()).unwrap_or_else(|_| {
                            error!("Failed to send sync response");
                        })
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn event_test() {
        #[derive(Debug, Clone)]
        struct Ping {
            i: i32,
        }
        impl Eventable for Ping {}

        async fn ping_handler_a(event: Ping, _eve: Eve<()>) {
            println!("ping a {:?}", event.i);
        }

        async fn ping_handler_b(event: Ping, _eve: Eve<()>) {
            println!("ping b {:?}", event.i);
        }

        let eve = EveBuilder::new(())
            .reg_handler(ping_handler_a)
            .unwrap()
            .reg_handler(ping_handler_b)
            .unwrap()
            .build()
            .unwrap();
        eve.dispatch(Ping { i: 10 }).await.unwrap();
        println!("dispatched");

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
