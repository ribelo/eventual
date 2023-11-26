use std::{fmt, future::Future, marker::PhantomData, ops::Deref, sync::Arc};

use async_trait::async_trait;
use dyn_clone::DynClone;

use crate::{
    eve::Eve,
    event::{Event, Eventable},
    reactive::Node,
    BoxableValue,
};

#[derive(Clone)]
pub struct EventContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    pub(crate) event: Arc<dyn Eventable>,
    pub(crate) eve: &'a Eve<S>,
}

impl<'a, S> EventContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn new(event: Arc<dyn Eventable>, eve: &'a Eve<S>) -> Self {
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
pub trait EventHandler<S, T>: Send + Sync + DynClone
where
    S: Send + Sync + Clone + 'static,
{
    async fn call(&self, context: &EventContext<S>);
}
dyn_clone::clone_trait_object!(<S, T> EventHandler< S, T>);

macro_rules! tuple_impls {
    ($($t:ident),*; $f:ident) => {
        #[async_trait]
        impl<S, $($t),*, $f, Fut> EventHandler< S, ($($t,)*)> for $f
        where
            S: Send + Sync + Clone + 'static,
            $f: Fn($($t),*) -> Fut + Send + Sync + Clone,
            $($t: FromEventContext<S>,)*
            Fut: Future<Output = ()> + Send,
        {
            async fn call(&self, context: &EventContext<S>) {
                (self)($(<$t>::from_context(&context).await,)*).await;
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

pub(crate) struct EventHandlerWrapper<E, S, H: EventHandler<S, T> + Copy, T>
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
    H: EventHandler<S, T> + Copy,
    T: Send + Sync,
{
}

impl<E, S, H, T> Clone for EventHandlerWrapper<E, S, H, T>
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
    H: EventHandler<S, T> + Copy,
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        *self
    }
}

#[async_trait]
impl<E, S, H, T> EventHandlerFn<S> for EventHandlerWrapper<E, S, H, T>
where
    E: Eventable,
    S: Send + Sync + Clone + 'static,
    H: EventHandler<S, T> + Copy,
    T: Send + Sync,
{
    async fn call_with_context(&self, context: &EventContext<S>) {
        self.handler.call(context).await;
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
        State(context.eve.app.clone())
    }
}

#[async_trait]
impl<S, T: BoxableValue + Clone> FromEventContext<S> for Node<T>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &EventContext<S>) -> Self {
        Node(context.eve.get_node::<T>().await.unwrap())
    }
}

#[async_trait]
impl<S, T> FromEventContext<S> for Event<T>
where
    S: Send + Sync + Clone + 'static,
    T: Eventable,
{
    async fn from_context(context: &EventContext<S>) -> Self {
        let event = context
            .event
            .clone()
            .downcast_arc::<T>()
            .unwrap_or_else(|_| panic!("Failed to downcast event"));
        Event(event)
    }
}
