use std::{fmt, future::Future, marker::PhantomData};

use async_trait::async_trait;
use dyn_clone::DynClone;
use rustc_hash::FxHashSet as HashSet;

use crate::{eve::Eve, id::Id, reactive::Node, BoxableValue};

#[derive(Clone)]
pub struct EffectContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    eve: &'a Eve<S>,
}

impl<'a, S> EffectContext<'a, S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn new(eve: &'a Eve<S>) -> Self {
        EffectContext { eve }
    }
}

#[async_trait]
pub trait FromEffectContext<S>: Send + Sync + 'static
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &mut EffectContext<S>) -> Self;
    fn collect_dependencies(_deps: &mut HashSet<Id>) {}
}

#[async_trait]
pub trait EffectHandler<S, T, R>: Send + Sync + DynClone
where
    S: Send + Sync + Clone + 'static,
{
    async fn call(&self, context: &mut EffectContext<S>) -> R;
    fn collect_dependencies(&self, deps: &mut HashSet<Id>);
}
dyn_clone::clone_trait_object!(<S, T, R> EffectHandler<S, T, R>);

#[async_trait]
impl<S, R, F, Fut> EffectHandler<S, (), R> for F
where
    S: Send + Sync + Clone + 'static,
    F: Fn() -> Fut + Send + Sync + Clone,
    Fut: Future<Output = R> + Send,
{
    async fn call(&self, _context: &mut EffectContext<S>) -> R {
        (self)().await
    }

    fn collect_dependencies(&self, _deps: &mut HashSet<Id>) {}
}

macro_rules! tuple_impls {
    ($($t:ident),*; $f:ident) => {
        #[async_trait]
        impl<S, $($t),*, R, $f, Fut> EffectHandler<S, ($($t,)*), R> for $f
        where
            S: Send + Sync + Clone + 'static,
            $f: Fn($($t),*) -> Fut + Send + Sync + Clone,
            $($t: FromEffectContext<S>,)*
            Fut: Future<Output = R> + Send,
        {
            async fn call(&self, context: &mut EffectContext<S>) -> R {
                (self)($(<$t>::from_context(context).await,)*).await
            }
            fn collect_dependencies(&self, deps: &mut HashSet<Id>) {
                $(
                    <$t as FromEffectContext<S>>::collect_dependencies(deps);
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
pub trait EffectHandlerFn<S, R>: Send + Sync + DynClone
where
    S: Send + Sync + Clone + 'static,
    R: Send + Sync,
{
    async fn call_with_context(&self, context: &mut EffectContext<S>);
    async fn collect_dependencies(&self, deps: &mut HashSet<Id>);
}
dyn_clone::clone_trait_object!(<S,R> EffectHandlerFn<S,R>);

impl<S, R> fmt::Debug for dyn EffectHandlerFn<S, R>
where
    R: Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommonFn").finish()
    }
}

pub(crate) struct EffectHandlerWrapper<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Copy,
{
    pub handler: H,
    pub phantom: PhantomData<(S, T, R)>,
}

impl<S, H, T, R> Copy for EffectHandlerWrapper<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Copy,
    T: Send + Sync,
{
}

impl<S, H, T, R> Clone for EffectHandlerWrapper<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Copy,
    T: Send + Sync,
{
    fn clone(&self) -> Self {
        *self
    }
}

#[async_trait]
impl<S, H, T, R> EffectHandlerFn<S, R> for EffectHandlerWrapper<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Copy,
    T: Send + Sync,
    R: Send + Sync,
{
    async fn call_with_context(&self, context: &mut EffectContext<S>) {
        self.handler.call(context).await;
    }
    async fn collect_dependencies(&self, deps: &mut HashSet<Id>) {
        self.handler.collect_dependencies(deps);
    }
}

#[async_trait]
impl<S> FromEffectContext<S> for Eve<S>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &mut EffectContext<S>) -> Self {
        context.eve.clone()
    }
}

#[async_trait]
impl<S, T: BoxableValue + Clone> FromEffectContext<S> for Node<T>
where
    S: Send + Sync + Clone + 'static,
{
    async fn from_context(context: &mut EffectContext<S>) -> Self {
        Node(context.eve.get_node::<T>().await.unwrap())
    }

    fn collect_dependencies(deps: &mut HashSet<Id>) {
        deps.insert(Id::new::<T>());
    }
}
