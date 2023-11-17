use rustc_hash::FxHashSet as HashSet;
use std::{fmt, future::Future, marker::PhantomData, ops::Deref};

use async_trait::async_trait;
use dyn_clone::DynClone;

use crate::{eve::Eve, event::Event, id::Id};

#[derive(Clone)]
pub struct EventContext {
    pub(crate) event: Option<Box<dyn Event>>,
    pub(crate) eventual: Eve,
}

impl EventContext {
    pub fn new(event: Box<dyn Event>, eventual: Eve) -> Self {
        EventContext {
            event: Some(event),
            eventual,
        }
    }
}

pub trait FromEventContext: Send + Sync + 'static {
    fn from_context(context: &EventContext) -> Self;

    #[allow(unused_variables)]
    fn collect_dependencies(deps: &mut HashSet<Id>) {}
}

#[async_trait]
pub trait EventHandler<T>: Send + Sync + DynClone {
    async fn call(&self, context: &EventContext);
    fn collect_dependencies(&self, deps: &mut HashSet<Id>);
}
dyn_clone::clone_trait_object!(<T> EventHandler<T>);

macro_rules! tuple_impls {
    ($($t:ident),*; $f:ident) => {
        #[async_trait]
        impl<$($t),*, $f, Fut> EventHandler<($($t,)*)> for $f
        where
            $f: Fn($($t),*) -> Fut + Send + Sync + Clone,
            $($t: FromEventContext,)*
            Fut: Future<Output = ()> + Send,
        {
            async fn call(&self, context: & EventContext) {
                (self)($(<$t>::from_context(&context),)*).await;
            }
            fn collect_dependencies(&self, deps: &mut HashSet<Id>) {
                $(
                    <$t as FromEventContext>::collect_dependencies(deps);
                )*
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
pub trait EventHandlerFn: Send + Sync + DynClone {
    async fn call_with_context(&self, context: &EventContext);
}
dyn_clone::clone_trait_object!(EventHandlerFn);

impl fmt::Debug for dyn EventHandlerFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommonFn").finish()
    }
}

pub(crate) struct EventHandlerWrapper<H: EventHandler<T> + Copy, T> {
    pub handler: H,
    pub phantom: PhantomData<T>,
}

impl<H, T> Copy for EventHandlerWrapper<H, T>
where
    H: EventHandler<T> + Copy,
    T: Send + Sync,
{
}

impl<H, T> Clone for EventHandlerWrapper<H, T>
where
    H: EventHandler<T> + Copy,
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        *self
    }
}

#[async_trait]
impl<H, T> EventHandlerFn for EventHandlerWrapper<H, T>
where
    H: EventHandler<T> + Copy,
    T: Send + Sync,
{
    async fn call_with_context(&self, context: &EventContext) {
        self.handler.call(context).await;
    }
}

impl FromEventContext for Eve {
    fn from_context(context: &EventContext) -> Self {
        context.eventual.clone()
    }
}

#[derive(Debug)]
pub struct Evt<E: Event>(pub E);

impl<E: Event> Deref for Evt<E> {
    type Target = E;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<E: Event + Clone> FromEventContext for Evt<E> {
    fn from_context(context: &EventContext) -> Self {
        Evt(context
            .event
            .as_ref()
            .unwrap()
            .downcast_ref::<E>()
            .unwrap()
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fn() {
        pub struct Param(pub String);

        pub struct Uid(pub u32);

        impl FromEventContext for Param {
            fn from_context(_context: &EventContext) -> Self {
                Param("foo".to_string())
            }
        }

        impl FromEventContext for Uid {
            fn from_context(_context: &EventContext) -> Self {
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
        impl Event for Ping {}

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
