use arc_swap::ArcSwap;
use downcast_rs::DowncastSync;
use dyn_clone::DynClone;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    any::{type_name, Any},
    marker::PhantomData,
    mem,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::mpsc;

use crate::{
    error::NodeNotFoundError,
    event::{Event, Message},
    event_handler::{EventContext, EventHandler, EventHandlerFn, EventHandlerWrapper},
    id::Id,
    reactive::{NodeState, NodeValue, Reactive},
    BoxableValue,
};

#[derive(Clone, Debug)]
pub struct Eve {
    pub(crate) message_tx: mpsc::UnboundedSender<Message>,
    pub(crate) handlers: Arc<Mutex<HashMap<Id, Vec<Box<dyn EventHandlerFn>>>>>,

    //Reactively
    pub(crate) statuses: Arc<Mutex<HashMap<Id, NodeState>>>,
    pub(crate) sources: Arc<Mutex<HashMap<Id, ArcSwap<HashSet<Id>>>>>,
    pub(crate) subscribers: Arc<Mutex<HashMap<Id, ArcSwap<HashSet<Id>>>>>,
    pub(crate) reactive: Arc<Mutex<HashMap<Id, Arc<dyn Reactive>>>>,
    pub(crate) values: Arc<Mutex<HashMap<Id, NodeValue>>>,
    pub(crate) effects: Arc<Mutex<HashSet<Id>>>,
}

impl Default for Eve {
    fn default() -> Self {
        Self::new()
    }
}

impl Eve {
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let eve = Self {
            message_tx,
            handlers: Default::default(),
            statuses: Default::default(),
            sources: Default::default(),
            subscribers: Default::default(),
            reactive: Default::default(),
            values: Default::default(),
            effects: Default::default(),
        };
        run_events_loop(message_rx, eve.clone());
        eve
    }

    pub fn reg_handler<E, T, H>(&mut self, handler: H) -> Result<(), NodeNotFoundError>
    where
        E: Event,
        T: Send + Sync + 'static,
        H: EventHandler<T> + Copy + 'static,
    {
        let mut deps = HashSet::default();
        handler.collect_dependencies(&mut deps);

        for node_id in deps {
            if !self.statuses.lock().unwrap().contains_key(&node_id) {
                return Err(NodeNotFoundError { id: node_id });
            }
        }

        let id = Id::new::<E>();
        let wrapper = EventHandlerWrapper {
            handler,
            phantom: PhantomData::<T>,
        };
        self.handlers
            .lock()
            .unwrap()
            .entry(id)
            .or_default()
            .push(Box::new(wrapper));

        Ok(())
    }

    pub fn get_handlers(&self, id: Id) -> Option<Vec<Box<dyn EventHandlerFn>>> {
        self.handlers.lock().unwrap().get(&id).cloned()
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
    async fn it_works() {
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

        let mut eve = Eve::new();
        eve.reg_handler::<Ping, _, _>(ping_handler_a);
        eve.reg_handler::<Ping, _, _>(ping_handler_b);
        eve.dispatch(Ping { i: 10 });
        println!("dispatched");

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
