use rustc_hash::FxHashSet as HashSet;
use std::{fmt, future::Future, marker::PhantomData, ops::Deref, sync::Arc};

use async_trait::async_trait;
use dyn_clone::DynClone;

use crate::{eve::Eve, event::Eventable, id::Id, reactive::Node, BoxableValue};

#[derive(Clone)]
pub struct EventContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    pub(crate) event: Box<dyn Eventable>,
    pub(crate) eve: &'a Eve<S>,
}

impl<'a, S> EventContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn new(event: Box<dyn Eventable>, eve: &'a Eve<S>) -> Self {
        EventContext { event, eve }
    }
}

#[async_trait]
pub trait FromEventContext<S>: Send + Sync + 'static
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &EventContext<S>) -> Self;
}

#[async_trait]
pub trait EventHandler<E, S, T>: Send + Sync + DynClone
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
{
    async fn call(&self, event: E, context: &EventContext<S>);
}
dyn_clone::clone_trait_object!(<E, S, T> EventHandler<E, S, T>);

macro_rules! tuple_impls {
    ($($t:ident),*; $f:ident) => {
        #[async_trait]
        impl<E, S, $($t),*, $f, Fut> EventHandler<E, S, ($($t,)*)> for $f
        where
            E: Eventable,
            S: Send + Sync + Clone + 'static,
            $f: Fn(E, $($t),*) -> Fut + Send + Sync + Clone,
            $($t: FromEventContext<S>,)*
            Fut: Future<Output = ()> + Send,
        {
            async fn call(&self, event: E, context: &EventContext<S>) {
                (self)(event, $(<$t>::from_context(&context).await,)*).await;
            }
        }
    }
}

macro_rules! impl_handler {
    (($($t:ident),*), $f:ident) => {
        tuple_impls!($($t),*; $f);
    };
}

impl_handler!((T1), F);
impl_handler!((T1, T2), F);
impl_handler!((T1, T2, T3), F);
impl_handler!((T1, T2, T3, T4), F);
impl_handler!((T1, T2, T3, T4, T5), F);
impl_handler!((T1, T2, T3, T4, T5, T6), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7, T8), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7, T8, T9), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7, T8, T9, T10), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11), F);
impl_handler!((T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12), F);

#[async_trait]
pub trait EventHandlerFn<S>: Send + Sync + DynClone
where
    S: Send + Sync + Clone + 'static,
{
    async fn call_with_context(&self, context: &EventContext<S>);
}
dyn_clone::clone_trait_object!(<S> EventHandlerFn<S>);

impl<S> fmt::Debug for dyn EventHandlerFn<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommonFn").finish()
    }
}

pub(crate) struct EventHandlerWrapper<E, S, H: EventHandler<E, S, T> + Copy, T>
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
{
    pub handler: H,
    pub phantom: PhantomData<(E, S, T)>,
}

impl<E, S, H, T> Copy for EventHandlerWrapper<E, S, H, T>
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
    H: EventHandler<E, S, T> + Copy,
    T: Send + Sync,
{
}

impl<E, S, H, T> Clone for EventHandlerWrapper<E, S, H, T>
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
    H: EventHandler<E, S, T> + Copy,
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        *self
    }
}

#[async_trait]
impl<E, S, H, T> EventHandlerFn<S> for EventHandlerWrapper<E, S, H, T>
where
    E: Eventable + Clone,
    S: Send + Sync + Clone + 'static,
    H: EventHandler<E, S, T> + Copy,
    T: Send + Sync,
{
    async fn call_with_context(&self, context: &EventContext<S>) {
        let event = context.event.clone().downcast::<E>().unwrap();
        self.handler.call(*event, context).await;
    }
}

#[async_trait]
impl<S> FromEventContext<S> for Eve<S>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &EventContext<S>) -> Self {
        context.eve.clone()
    }
}

pub struct State<S: Send + Sync + Clone + 'static>(pub S);

impl<S: Send + Sync + Clone + 'static> Deref for State<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
impl<S> FromEventContext<S> for State<S>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &EventContext<S>) -> Self {
        State(context.eve.state.clone())
    }
}

#[async_trait]
impl<S, T: BoxableValue + Clone> FromEventContext<S> for Node<T>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &EventContext<S>) -> Self {
        println!("type: {:?}", std::any::type_name::<T>());
        Node(context.eve.get_node::<T>().await.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fn() {
        pub struct Param(pub String);

        pub struct Uid(pub u32);

        #[async_trait]
        impl FromEventContext<()> for Param {
            async fn from_context(_context: &EventContext<()>) -> Self {
                Param("foo".to_string())
            }
        }

        #[async_trait]
        impl FromEventContext<()> for Uid {
            async fn from_context(_context: &EventContext<()>) -> Self {
                Uid(7)
            }
        }
        async fn print_id(id: Uid) {
            println!("id is {}", id.0);
        }
        async fn print_param(Param(param): Param) {
            println!("param is {param}");
        }

        async fn print_all(Param(param): Param, Uid(id): Uid) {
            println!("param is {param}, id is {id}");
        }

        async fn print_all_switched(Uid(id): Uid, Param(param): Param) {
            println!("param is {param}, id is {id}");
        }

        #[derive(Debug, Clone)]
        struct Ping;
        impl Eventable for Ping {}

        // let eve = Eve::new();
        // let context = EventContext::new(Box::new(Ping), eve);
        //
        // pub(crate) async fn trigger<T, H>(context: &EventContext, handler: H)
        // where
        //     H: EventHandler<T>,
        // {
        //     handler.call(context).await;
        // }
        //
        // trigger(&context, print_id).await;
        // trigger(&context, print_param).await;
        // trigger(&context, print_all).await;
        // trigger(&context, print_all_switched).await;
    }
}
