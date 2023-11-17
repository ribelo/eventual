use arc_swap::ArcSwap;
use downcast_rs::DowncastSync;
use dyn_clone::DynClone;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    any::{type_name, Any},
    borrow::BorrowMut,
    cell::RefCell,
    marker::PhantomData,
    mem,
    rc::Rc,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::mpsc;

use crate::{
    error::{NodeNotFoundError, RunEveError},
    event::{Event, Message},
    event_handler::{EventContext, EventHandler, EventHandlerFn, EventHandlerWrapper},
    id::Id,
    reactive::{NodeState, NodeValue, Reactive},
    BoxableValue,
};

#[derive(Clone, Debug)]
pub struct Eve {
    pub(crate) message_tx: mpsc::UnboundedSender<Message>,
    pub(crate) handlers: Arc<HashMap<Id, Vec<Box<dyn EventHandlerFn>>>>,

    //Reactively
    pub(crate) statuses: Arc<HashMap<Id, Arc<Mutex<NodeState>>>>,
    pub(crate) sources: Arc<HashMap<Id, Arc<HashSet<Id>>>>,
    pub(crate) subscribers: Arc<HashMap<Id, Arc<HashSet<Id>>>>,
    pub(crate) reactive: Arc<HashMap<Id, Arc<dyn Reactive>>>,
    pub(crate) values: Arc<HashMap<Id, Arc<Mutex<NodeValue>>>>,
    pub(crate) effects: Arc<HashSet<Id>>,
}

pub struct EveBuilder {
    pub(crate) message_tx: mpsc::UnboundedSender<Message>,
    pub(crate) message_rx: Option<mpsc::UnboundedReceiver<Message>>,
    pub(crate) handlers: HashMap<Id, Vec<Box<dyn EventHandlerFn>>>,
    pub(crate) statuses: HashMap<Id, NodeState>,
    pub(crate) sources: HashMap<Id, HashSet<Id>>,
    pub(crate) subscribers: HashMap<Id, HashSet<Id>>,
    pub(crate) reactive: HashMap<Id, Arc<dyn Reactive>>,
    pub(crate) values: HashMap<Id, NodeValue>,
    pub(crate) effects: HashSet<Id>,
}

impl EveBuilder {
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Self {
            message_tx,
            message_rx: Some(message_rx),
            handlers: Default::default(),
            statuses: Default::default(),
            sources: Default::default(),
            subscribers: Default::default(),
            reactive: Default::default(),
            values: Default::default(),
            effects: Default::default(),
        }
    }

    pub fn build(&mut self) -> Result<Eve, RunEveError> {
        for deps in self.sources.values() {
            for dep_id in deps.iter() {
                if self.sources.contains_key(dep_id) {
                    return Err(NodeNotFoundError { id: *dep_id }.into());
                }
            }
        }

        let eve = Eve {
            message_tx: self.message_tx.clone(),
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
                    .map(|(k, v)| (k, Arc::new(v))) // Clone the HashSet and wrap it in Arc
                    .collect(),
            ),
            reactive: Arc::new(mem::take(&mut self.reactive.clone())), // Clone before taking
            values: Arc::new(
                self.values
                    .drain()
                    .map(|(k, v)| (k, Arc::new(Mutex::new(v))))
                    .collect(),
            ),
            effects: Arc::new(mem::take(&mut self.effects.clone())), // Clone before taking
        };

        run_events_loop(self.message_rx.take().unwrap(), eve.clone());

        Ok(eve)
    }

    pub fn reg_handler<E, T, H>(mut self, handler: H) -> Result<Self, NodeNotFoundError>
    where
        E: Event,
        T: Send + Sync + 'static,
        H: EventHandler<T> + Copy + 'static,
    {
        let mut deps = HashSet::default();
        handler.collect_dependencies(&mut deps);

        let id = Id::new::<E>();
        let wrapper = EventHandlerWrapper {
            handler,
            phantom: PhantomData::<T>,
        };
        self.handlers.entry(id).or_default().push(Box::new(wrapper));

        Ok(self)
    }
}

impl Eve {
    pub fn get_handlers(&self, id: Id) -> Option<Vec<Box<dyn EventHandlerFn>>> {
        self.handlers.get(&id).cloned()
    }

    pub fn dispatch<E: Event>(&self, event: E) {
        self.message_tx.send(Message::new(event)).unwrap();
    }
}

fn run_events_loop(mut message_rx: mpsc::UnboundedReceiver<Message>, eve: Eve) {
    tokio::spawn(async move {
        loop {
            while let Some(msg) = message_rx.recv().await {
                if let Some(handlers) = eve.get_handlers(msg.id) {
                    let ctx = EventContext::new(msg.event, eve.clone());
                    for handler in handlers {
                        handler.call_with_context(&ctx).await;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::event_handler::Evt;

    use super::*;

    #[tokio::test]
    async fn event_test() {
        #[derive(Debug, Clone)]
        struct Ping {
            i: i32,
        };
        impl Event for Ping {}

        async fn ping_handler_a(eve: Eve, event: Evt<Ping>) {
            println!("ping a {:?}", event.i);
        }

        async fn ping_handler_b(eve: Eve, event: Evt<Ping>) {
            println!("ping b {:?}", event.i);
        }

        let eve = EveBuilder::new()
            .reg_handler::<Ping, _, _>(ping_handler_a)
            .unwrap()
            .reg_handler::<Ping, _, _>(ping_handler_b)
            .unwrap()
            .build()
            .unwrap();
        eve.dispatch(Ping { i: 10 });
        println!("dispatched");

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
