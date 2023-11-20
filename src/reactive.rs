use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use dyn_clone::DynClone;
use rustc_hash::FxHashSet as HashSet;
use tokio::sync::{mpsc, oneshot};

use crate::{
    effect_handler::{EffectContext, EffectHandler, EffectHandlerFn},
    error::{
        DowncastError, GetNodeValueError, NodeNotFoundError, ReactivelyChannelError,
        ReplyChannelError, SetNodeValueError,
    },
    eve::{Eve, EveBuilder},
    id::Id,
    BoxableValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Clean,
    Check,
    Dirty,
}

pub trait IntoNodeValue {
    fn into_node_value(self) -> NodeValue;
}

impl<T> IntoNodeValue for T
where
    T: BoxableValue + Clone + 'static,
{
    fn into_node_value(self) -> NodeValue {
        NodeValue::Memoized(Box::new(self))
    }
}

#[derive(Debug, Clone)]
pub enum NodeValue {
    Uninitialized,
    Void,
    Memoized(Box<dyn BoxableValue>),
}

impl NodeValue {
    pub fn new<T>(value: T) -> Self
    where
        T: BoxableValue,
    {
        Self::Memoized(Box::new(value))
    }
    pub fn get<T>(&self) -> Option<T>
    where
        T: BoxableValue + Clone,
    {
        match self {
            NodeValue::Uninitialized | NodeValue::Void => None,
            NodeValue::Memoized(value) => value.downcast_ref::<T>().cloned(),
        }
    }
}

pub struct Node<T: BoxableValue + Clone + 'static>(pub T);

impl<S> EveBuilder<S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn reg_reactive(
        &mut self,
        id: Id,
        sources: HashSet<Id>,
        value: NodeValue,
        reactive: Arc<dyn Reactive<S>>,
    ) {
        self.statuses.insert(id, NodeState::Dirty);

        for node_id in sources.iter() {
            let mut subs = self.subscribers.entry(*node_id).or_default().clone();
            subs.insert(id);
            self.subscribers.insert(*node_id, subs);
        }

        self.sources.insert(id, sources);

        self.subscribers.insert(id, Default::default());

        self.values.insert(id, value);

        self.reactive.insert(id, reactive);
    }

    pub fn reg_signal<F, R>(mut self, factory: F) -> Self
    where
        F: Fn() -> R + Send + Sync + Clone + 'static,
        R: BoxableValue + Clone + PartialEq + 'static,
    {
        let id = Id::new::<R>();
        let anchor: SignalAnchor<F, R> = SignalAnchor::new(factory);
        self.reg_reactive(
            id,
            Default::default(),
            NodeValue::Uninitialized,
            Arc::new(anchor),
        );

        self
    }

    pub fn reg_memo<H, T, R>(mut self, handler: H) -> Self
    where
        H: EffectHandler<S, T, R> + Clone + 'static,
        T: Send + Sync + 'static,
        R: BoxableValue + Clone + PartialEq + 'static,
    {
        let mut sources = HashSet::default();
        handler.collect_dependencies(&mut sources);

        let id = Id::new::<R>();
        let anchor: MemoAnchor<S, H, T, R> = MemoAnchor::new(handler);
        self.reg_reactive(id, sources, NodeValue::Uninitialized, Arc::new(anchor));

        self
    }

    pub fn reg_effect<H, T>(mut self, handler: H) -> Self
    where
        H: EffectHandler<S, T, ()> + Clone + 'static,
        T: Send + Sync + 'static,
    {
        let mut sources = HashSet::default();
        handler.collect_dependencies(&mut sources);

        let id = Id::new::<H>();
        println!("effect id: {:?} {}", id, id);
        let anchor: EffectAnchor<S, H, T> = EffectAnchor::new(handler);
        self.reg_reactive(id, sources, NodeValue::Void, Arc::new(anchor));
        self.effects.insert(id);

        self
    }
}

impl<S> Eve<S>
where
    S: Send + Sync + Clone + 'static,
{
    fn get_node_state(&self, id: Id) -> Option<NodeState> {
        self.statuses.get(&id).map(|state| *state.lock().unwrap())
    }
    fn set_node_state(&self, id: Id, state: NodeState) {
        if let Some(old_value) = self.statuses.get(&id) {
            *old_value.lock().unwrap() = state;
        }
    }
    pub async fn get_node<T>(&self) -> Option<T>
    where
        T: BoxableValue + Clone,
    {
        let id = Id::new::<T>();
        self.update_node_if_necessary(id).await;
        self.get_node_value::<T>()
    }

    pub(crate) async fn get_node_by_id(&self, id: Id) -> Option<Arc<Mutex<NodeValue>>> {
        self.update_node_if_necessary(id).await;
        self.get_node_boxed_value(id)
    }

    pub(crate) fn get_node_value<T>(&self) -> Option<T>
    where
        T: BoxableValue + Clone,
    {
        let id = Id::new::<T>();
        self.values
            .get(&id)
            .and_then(|value| value.lock().unwrap().get::<T>())
    }

    pub(crate) fn get_node_boxed_value(&self, id: Id) -> Option<Arc<Mutex<NodeValue>>> {
        self.values.get(&id).cloned()
    }

    pub(crate) fn set_node_value_by_id(&self, id: Id, value: NodeValue) {
        if let Some(node_value) = self.get_node_boxed_value(id) {
            *node_value.lock().unwrap() = value;
        }
    }

    fn get_reactive(&self, id: Id) -> Option<Arc<dyn Reactive<S>>> {
        self.reactive.get(&id).cloned()
    }

    fn get_node_sources(&self, id: Id) -> Option<Arc<HashSet<Id>>> {
        self.sources.get(&id).cloned()
    }

    fn get_node_subscribers(&self, id: Id) -> Option<Arc<HashSet<Id>>> {
        self.subscribers.get(&id).cloned()
    }

    async fn update_node(&self, id: Id) {
        if let Some(node) = self.get_reactive(id) {
            if node.changed(self).await {
                if let Some(subs) = self.get_node_subscribers(id) {
                    for sub_id in subs.iter() {
                        self.set_node_state(*sub_id, NodeState::Dirty);
                    }
                }
            }
            self.set_node_state(id, NodeState::Clean);
        }
    }

    async fn update_node_if_necessary(&self, root_id: Id) {
        let state = self.get_node_state(root_id);
        match state {
            None | Some(NodeState::Clean) => return,
            _ => {}
        }

        let mut levels: BTreeMap<usize, HashSet<Id>> = BTreeMap::default();
        let mut queue: Vec<(usize, Id)> = vec![(0, root_id)];

        while let Some((level, id)) = queue.pop() {
            levels.entry(level).or_default().insert(id);
            if let Some(sources) = self.get_node_sources(id) {
                let next_level = level + 1;
                for &source_id in sources.iter() {
                    queue.push((next_level, source_id));
                }
            }
        }

        while let Some((_, ids)) = levels.pop_last() {
            // let mut tasks = Vec::new();
            for id in ids {
                self.update_node(id).await;
                // let cloned_self = self.clone();

                // TODO:

                // let task = self
                //     .executor
                //     .spawn(async move { cloned_self.update_node(id).await });
                // tasks.push(task);
            }
            // for task in tasks {
            //     task.await.unwrap();
            // }
        }
    }

    fn mark_node_as_check(&self, id: Id) {
        self.set_node_state(id, NodeState::Check);

        if let Some(subs) = self.get_node_subscribers(id) {
            for sub_id in subs.iter() {
                self.mark_node_as_check(*sub_id);
            }
        }
    }

    pub(crate) async fn mark_node_as_dirty(&self, id: Id) {
        self.set_node_state(id, NodeState::Dirty);

        if let Some(subs) = self.get_node_subscribers(id) {
            for sub_id in subs.iter() {
                self.mark_node_as_check(*sub_id);
            }
        }
        self.fire_pending_effects().await;
    }

    async fn fire_pending_effects(&self) {
        let effects: Vec<Id> = self.effects.iter().copied().collect();
        for effect_id in effects.iter() {
            if let Some(NodeState::Dirty) = self.get_node_state(*effect_id) {
                self.update_node_if_necessary(*effect_id).await;
            }
        }
    }

    pub fn get_signal<R>(&self) -> Option<Signal<S, R>>
    where
        R: BoxableValue + Clone + PartialEq + 'static,
    {
        let id = Id::new::<R>();
        if self.statuses.contains_key(&id) {
            Some(Signal::new(self.clone()))
        } else {
            None
        }
    }

    pub fn get_memo<R>(&self) -> Option<Memo<S, R>>
    where
        R: BoxableValue + Clone + PartialEq + 'static,
    {
        let id = Id::new::<R>();
        if self.statuses.contains_key(&id) {
            Some(Memo::new(self.clone()))
        } else {
            None
        }
    }
}

pub trait Observable: Send + Sync + Clone + fmt::Debug + 'static {}
impl<T: Send + Sync + Clone + fmt::Debug + 'static> Observable for T {}

#[async_trait]
pub trait Reactive<S>: DynClone + Send + Sync + 'static
where
    S: Send + Sync,
{
    async fn changed(&self, eve: &Eve<S>) -> bool;
}
dyn_clone::clone_trait_object!(<S> Reactive<S>);

impl<S> fmt::Debug for dyn Reactive<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reactive").finish()
    }
}

#[derive(Clone)]
pub struct TriggerAnchor {
    pub id: Id,
}

impl TriggerAnchor {
    pub fn new<T: Observable>() -> Self {
        Self { id: Id::new::<T>() }
    }
}

#[async_trait]
impl<S: Send + Sync> Reactive<S> for TriggerAnchor {
    async fn changed(&self, _eve: &Eve<S>) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct Signal<S, R>
where
    S: Send + Sync,
{
    pub id: Id,
    phantom: PhantomData<R>,
    eve: Eve<S>,
}

impl<S, R> Signal<S, R>
where
    R: BoxableValue + Clone + PartialEq + 'static,
    S: Send + Sync + Clone + 'static,
{
    pub fn new(eve: Eve<S>) -> Self {
        Self {
            id: Id::new::<R>(),
            phantom: PhantomData,
            eve,
        }
    }
    pub async fn get(&self) -> R {
        self.eve.get_node::<R>().await.unwrap()
    }
    pub async fn set(&self, value: R) {
        let old_value = self.get().await;
        if value != old_value {
            self.eve
                .set_node_value_by_id(self.id, value.into_node_value());
            self.eve.mark_node_as_dirty(self.id).await;
        }
    }
    pub async fn update<U>(&self, f: U)
    where
        U: FnOnce(&R) -> Cow<'static, R> + Send + Sync + 'static,
    {
        let old_value = self.get().await;
        match f(&old_value) {
            Cow::Borrowed(_) => {}
            Cow::Owned(new_value) => {
                self.set(new_value).await;
            }
        }
    }
}

pub struct SignalAnchor<F, R>
where
    F: Fn() -> R + Send + Sync + Clone + 'static,
{
    pub id: Id,
    handler: F,
    phantom: PhantomData<R>,
}

impl<F, R> SignalAnchor<F, R>
where
    F: Fn() -> R + Send + Sync + Clone + 'static,
    R: BoxableValue + Clone + 'static,
{
    pub fn new(handler: F) -> Self {
        Self {
            id: Id::new::<R>(),
            handler,
            phantom: PhantomData,
        }
    }
}

impl<F, R> Clone for SignalAnchor<F, R>
where
    F: Fn() -> R + Send + Sync + Clone + 'static,
    R: BoxableValue + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            handler: self.handler.clone(),
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<S, F, R> Reactive<S> for SignalAnchor<F, R>
where
    S: Send + Sync + Clone + 'static,
    F: Fn() -> R + Send + Sync + Clone + 'static,
    R: BoxableValue + Clone + 'static,
{
    async fn changed(&self, eve: &Eve<S>) -> bool {
        let node_value = eve.get_node_boxed_value(self.id).expect("node not found");
        let mut guard = node_value.lock().unwrap();
        if let NodeValue::Uninitialized = *guard {
            let value = (self.handler)();
            *guard = value.into_node_value();
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct Memo<S, R>
where
    S: Send + Sync + Clone + 'static,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    pub id: Id,
    phantom: PhantomData<R>,
    eve: Eve<S>,
}

impl<S, R> Memo<S, R>
where
    S: Send + Sync + Clone + 'static,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    pub fn new(eve: Eve<S>) -> Self {
        Self {
            id: Id::new::<R>(),
            phantom: PhantomData,
            eve,
        }
    }
    pub async fn get(&self) -> R {
        self.eve.get_node::<R>().await.unwrap()
    }
}

pub struct MemoAnchor<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R>,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    pub id: Id,
    handler: H,
    phantom: PhantomData<(S, T, R)>,
}

impl<S, H, T, R> MemoAnchor<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Clone,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    pub fn new(handler: H) -> Self {
        Self {
            id: Id::new::<R>(),
            handler,
            phantom: PhantomData,
        }
    }
}

impl<S, H, T, R> Clone for MemoAnchor<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, R> + Clone,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            handler: self.handler.clone(),
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<S, H, T, R> Reactive<S> for MemoAnchor<S, H, T, R>
where
    S: Send + Sync + Clone + 'static,
    T: Send + Sync + 'static,
    H: EffectHandler<S, T, R> + Clone + 'static,
    R: BoxableValue + Clone + PartialEq + 'static,
{
    async fn changed(&self, eve: &Eve<S>) -> bool {
        let mut context = EffectContext::new(eve);
        let new_value = self.handler.call(&mut context).await;
        if let Some(old_value) = eve.get_node_value::<R>() {
            if new_value != old_value {
                eve.set_node_value_by_id(self.id, NodeValue::Memoized(Box::new(new_value)));
                true
            } else {
                false
            }
        } else {
            eve.set_node_value_by_id(self.id, NodeValue::Memoized(Box::new(new_value)));
            true
        }
    }
}

pub struct EffectAnchor<S, H, T>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, ()>,
{
    pub id: Id,
    handler: H,
    phantom: PhantomData<(S, T)>,
}

impl<S, H, T> Clone for EffectAnchor<S, H, T>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, ()> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            handler: self.handler.clone(),
            phantom: PhantomData,
        }
    }
}

impl<S, H, T> EffectAnchor<S, H, T>
where
    S: Send + Sync + Clone + 'static,
    H: EffectHandler<S, T, ()> + Clone + 'static,
{
    pub fn new(handler: H) -> Self {
        Self {
            id: Id::new::<H>(),
            handler,
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<S, H, T> Reactive<S> for EffectAnchor<S, H, T>
where
    S: Send + Sync + Clone + 'static,
    T: Send + Sync + 'static,
    H: EffectHandler<S, T, ()> + Clone + 'static,
{
    async fn changed(&self, eve: &Eve<S>) -> bool {
        let mut context = EffectContext::new(&eve);
        self.handler.call(&mut context).await;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        eve::{Eve, EveBuilder},
        id::Id,
    };

    #[tokio::test]
    async fn test_signal() {
        let mut eve_builder = EveBuilder::new(());

        let eve = eve_builder.reg_signal(|| 7_i32).build().unwrap();
        let signal = eve.get_signal::<i32>().unwrap();
        dbg!(signal.get().await);
        println!("3");
        signal.set(8).await;
        println!("4");
        dbg!(signal.get().await);
        dbg!(eve.get_node::<i32>().await.unwrap());
    }

    #[tokio::test]
    async fn test_memo() {
        async fn memo_fn(eve: Eve<()>) -> i32 {
            println!("memo");
            0
        }
        let eve = EveBuilder::new(()).reg_memo(memo_fn).build().unwrap();
        let memo = eve.get_memo::<i32>().unwrap();
        dbg!(memo.get().await);
    }

    #[tokio::test]
    async fn test_effect() {
        // let eve = Eve::new();
        //
        // async fn test_effect() {
        //     println!("foo test_effect");
        // }
        //
        // eve.add_effect(test_effect).unwrap();
        // eve.reg_effect(|eve: Eve| async move {
        //     println!("test_closure1");
        // })
        // .unwrap();
        // eve.reg_effect(|eve: Eve| async move {
        //     println!("test_closure2");
        // })
        // .unwrap();
    }
}
