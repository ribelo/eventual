use std::{fmt, future::Future, marker::PhantomData};

use async_trait::async_trait;
use dyn_clone::DynClone;
use rustc_hash::FxHashSet as HashSet;

use crate::{eve::Eve, id::Id, reactive::Node, BoxableValue};

#[derive(Clone)]
pub struct EffectContext<'a> {
    eve: &'a Eve,
}

impl<'a> EffectContext<'a> {
    pub fn new(eve: &'a Eve) -> Self {
        EffectContext { eve }
    }
}

pub trait FromEffectContext: Send + Sync + 'static {
    fn from_context(context: &mut EffectContext) -> Self;
    fn collect_dependencies(_deps: &mut HashSet<Id>) {}
}

#[async_trait]
pub trait EffectHandler<T, R>: Send + Sync + DynClone {
    async fn call(&self, context: &mut EffectContext) -> R;
    fn collect_dependencies(&self, deps: &mut HashSet<Id>);
}
dyn_clone::clone_trait_object!(<T, R> EffectHandler<T, R>);

#[async_trait]
impl<R, F, Fut> EffectHandler<(), R> for F
where
    F: Fn() -> Fut + Send + Sync + Clone,
    Fut: Future<Output = R> + Send,
{
    async fn call(&self, _context: &mut EffectContext) -> R {
        (self)().await
    }

    fn collect_dependencies(&self, _deps: &mut HashSet<Id>) {}
}

macro_rules! tuple_impls {
    ($($t:ident),*; $f:ident) => {
        #[async_trait]
        impl<$($t),*, R, $f, Fut> EffectHandler<($($t,)*), R> for $f
        where
            $f: Fn($($t),*) -> Fut + Send + Sync + Clone,
            $($t: FromEffectContext,)*
            Fut: Future<Output = R> + Send,
        {
            async fn call(&self, context: &mut EffectContext) -> R {
                (self)($(<$t>::from_context(context),)*).await
            }
            fn collect_dependencies(&self, deps: &mut HashSet<Id>) {
                $(
                    <$t as FromEffectContext>::collect_dependencies(deps);
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
pub trait EffectHandlerFn<R>: Send + Sync + DynClone
where
    R: Send + Sync,
{
    async fn call_with_context(&self, context: &mut EffectContext);
    async fn collect_dependencies(&self, deps: &mut HashSet<Id>);
}
dyn_clone::clone_trait_object!(<R> EffectHandlerFn<R>);

impl<R> fmt::Debug for dyn EffectHandlerFn<R>
where
    R: Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommonFn").finish()
    }
}

pub(crate) struct EffectHandlerWrapper<H, T, R>
where
    H: EffectHandler<T, R> + Copy,
{
    pub handler: H,
    pub phantom: PhantomData<(T, R)>,
}

impl<H, T, R> Copy for EffectHandlerWrapper<H, T, R>
where
    H: EffectHandler<T, R> + Copy,
    T: Send + Sync,
{
}

impl<H, T, R> Clone for EffectHandlerWrapper<H, T, R>
where
    H: EffectHandler<T, R> + Copy,
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        *self
    }
}

#[async_trait]
impl<H, T, R> EffectHandlerFn<R> for EffectHandlerWrapper<H, T, R>
where
    H: EffectHandler<T, R> + Copy,
    T: Send + Sync,
    R: Send + Sync,
{
    async fn call_with_context(&self, context: &mut EffectContext) {
        self.handler.call(context).await;
    }
    async fn collect_dependencies(&self, deps: &mut HashSet<Id>) {
        self.handler.collect_dependencies(deps);
    }
}

impl FromEffectContext for Eve {
    fn from_context(context: &mut EffectContext) -> Self {
        context.eve.clone()
    }
}

impl<T: BoxableValue + Clone> FromEffectContext for Node<T> {
    fn from_context(context: &mut EffectContext) -> Self {
        Node(context.eve.get_node_value::<T>().unwrap())
    }

    fn collect_dependencies(deps: &mut HashSet<Id>) {
        deps.insert(Id::new::<T>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn effect_handler_fn() {
        pub struct Param(pub String);

        pub struct Uid(pub u32);

        impl FromEffectContext for Param {
            fn from_context(_context: &mut EffectContext) -> Self {
                Param("foo".to_string())
            }
        }

        impl FromEffectContext for Uid {
            fn from_context(_context: &mut EffectContext) -> Self {
                Uid(7)
            }
        }
        async fn return_i32(_id: Uid) -> i32 {
            12
        }
        // let eve = Eve::new();
        // let context = EffectContext::new(&eve);
        //
        // pub(crate) async fn trigger<T, H, R>(mut context: EffectContext<'_>, handler: H) -> R
        // where
        //     H: EffectHandler<T, R>,
        // {
        //     handler.call(&mut context).await
        // }
        //
        // dbg!(trigger(context, return_i32).await);
    }
}
