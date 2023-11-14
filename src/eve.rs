use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    any::{type_name, Any},
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

use crate::{
    event::{Event, Message},
    id::Id,
    magic_handler::{CommonFn, Context, Handler, HandlerWrapper},
};

#[derive(Clone, Debug)]
pub struct Eventual {
    message_tx: mpsc::UnboundedSender<Message>,
    handlers: Arc<Mutex<HashMap<Id, Vec<Box<dyn CommonFn>>>>>,
}

impl Default for Eventual {
    fn default() -> Self {
        Self::new()
    }
}

impl Eventual {
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let eve = Self {
            message_tx,
            handlers: Default::default(),
        };
        run_events_loop(message_rx, eve.clone());
        eve
    }

    pub fn reg_handler<E, T, H>(&mut self, handler: H)
    where
        E: Event,
        T: Send + Sync + 'static,
        H: Handler<T> + Copy + 'static,
    {
        let id = Id::new::<E>();
        let wrapper = HandlerWrapper {
            handler,
            phantom: PhantomData::<T>,
        };
        self.handlers
            .lock()
            .unwrap()
            .entry(id)
            .or_default()
            .push(Box::new(wrapper));
    }

    pub fn dispatch<E: Event>(&self, event: E) {
        self.message_tx.send(Message::new(event)).unwrap();
    }
}

fn run_events_loop(mut message_rx: mpsc::UnboundedReceiver<Message>, eve: Eventual) {
    tokio::spawn(async move {
        loop {
            while let Some(msg) = message_rx.recv().await {
                let maybe_handlers = eve.handlers.lock().unwrap().get(&msg.id).cloned();
                if let Some(handlers) = maybe_handlers {
                    let ctx = Context::new(msg.event, eve.clone());
                    for handler in handlers {
                        let cloned_ctx = ctx.clone();
                        tokio::spawn(async move { handler.call_with_context(cloned_ctx).await });
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::magic_handler::{Eve, Evt};

    use super::*;

    #[tokio::test]
    async fn it_works() {
        #[derive(Debug, Clone)]
        struct Ping;
        impl Event for Ping {}

        async fn ping_handler(eve: Eve, event: Evt) {
            println!("ping {:?}", event);
        }

        let mut eve = Eventual::new();
        eve.reg_handler::<Ping, _, _>(ping_handler);
        eve.dispatch(Ping);

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
