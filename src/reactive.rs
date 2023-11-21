use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    ops::Deref,
    sync::{Arc, Mutex},
};

use async_recursion::async_recursion;
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
        NodeValue::Memoized(Arc::new(self))
    }
}

#[derive(Debug, Clone)]
pub enum NodeValue {
    Uninitialized,
    Void,
    Memoized(Arc<dyn BoxableValue>),
}

impl NodeValue {
    pub fn new<T>(value: T) -> Self
    where
        T: BoxableValue,
    {
        Self::Memoized(Arc::new(value))
    }
    pub fn get<T>(&self) -> Option<Arc<T>>
    where
        T: BoxableValue + Clone,
    {
        match self {
            NodeValue::Uninitialized | NodeValue::Void => None,
            NodeValue::Memoized(value) => value.clone().downcast_arc::<T>().ok(),
        }
    }
}

pub struct Node<T: BoxableValue + Clone + 'static>(pub Arc<T>);

impl<T> Deref for Node<T>
where
    T: BoxableValue + Clone + 'static,
{
    type Target = Arc<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

    pub fn reg_trigger<T>(mut self) -> Self
    where
        T: BoxableValue + Clone + Default + 'static,
    {
        let id = Id::new::<T>();
        let anchor: TriggerAnchor = TriggerAnchor::new::<T>();
        self.reg_reactive(
            id,
            Default::default(),
            NodeValue::Memoized(Arc::<T>::default()),
            Arc::new(anchor),
        );

        self
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

    pub async fn get_node<T>(&self) -> Option<Arc<T>>
    where
        T: BoxableValue + Clone,
    {
        let id = Id::new::<T>();
        self.update_node_if_necessary(id).await;
        self.get_node_value::<T>()
    }

    pub(crate) fn get_node_value<T>(&self) -> Option<Arc<T>>
    where
        T: BoxableValue + Clone,
    {
        let id = Id::new::<T>();
        self.values
            .lock()
            .unwrap()
            .get(&id)
            .and_then(|value| value.get::<T>())
    }

    pub(crate) fn get_node_boxed_value(&self, id: Id) -> Option<NodeValue> {
        self.values.lock().unwrap().get(&id).cloned()
    }

    pub async fn set_node_value<T>(&self, value: T)
    where
        T: BoxableValue + Clone,
    {
        let id = Id::new::<T>();
        self.set_node_value_by_id(id, NodeValue::new(value));
        self.mark_subs_as_dirty(id).await;
    }

    pub(crate) fn set_node_value_by_id(&self, id: Id, value: NodeValue) {
        self.values.lock().unwrap().insert(id, value);
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

    async fn update_node_if_necessary(&self, root_id: Id) {
        let state = self.get_node_state(root_id);
        if matches!(state, None | Some(NodeState::Clean)) {
            return;
        }

        let mut levels: BTreeMap<usize, HashSet<Id>> = BTreeMap::default();
        let mut queue: Vec<(usize, Id)> = vec![(0, root_id)];

        while let Some((level, id)) = queue.pop() {
            levels.entry(level).or_default().insert(id);
            if let Some(sources) = self.get_node_sources(id) {
                let next_level = level + 1;
                for &source_id in sources.iter() {
                    if self.get_node_state(source_id) == Some(NodeState::Dirty) {
                        queue.push((next_level, source_id));
                    }
                }
            }
        }

        while let Some((_, ids)) = levels.pop_last() {
            let mut set = tokio::task::JoinSet::new();
            for id in ids {
                if let Some(node) = self.get_reactive(id) {
                    let cloned_self = self.clone();
                    set.spawn(async move {
                        node.run(&cloned_self).await;
                    });
                }
            }
            while (set.join_next().await).is_some() {}
        }
    }

    pub(crate) async fn mark_subs_as_dirty(&self, root_id: Id) {
        let mut stack = Vec::new();

        if let Some(subs) = self.get_node_subscribers(root_id) {
            for &sub_id in subs.iter() {
                stack.push(sub_id);
            }
        }

        while let Some(id) = stack.pop() {
            self.set_node_state(id, NodeState::Dirty);

            if let Some(subs) = self.get_node_subscribers(id) {
                for &sub_id in subs.iter() {
                    stack.push(sub_id);
                }
            }
        }
    }

    async fn fire_pending_effects(&self) {
        let effects: Vec<Id> = self.effects.iter().copied().collect();
        for effect_id in effects.iter() {
            if let Some(NodeState::Dirty) = self.get_node_state(*effect_id) {
                self.update_node_if_necessary(*effect_id).await;
            }
        }
    }

    pub fn get_trigger<T>(&self) -> Option<Trigger<S, T>>
    where
        T: Send + Sync + Clone + 'static,
    {
        let id = Id::new::<T>();
        if self.statuses.contains_key(&id) {
            Some(Trigger::new(self.clone()))
        } else {
            None
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

#[async_trait]
pub trait Reactive<S>: DynClone + Send + Sync + 'static
where
    S: Send + Sync,
{
    async fn run(&self, eve: &Eve<S>);
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
    pub fn new<T: Send + Sync + 'static>() -> Self {
        Self { id: Id::new::<T>() }
    }
}

pub struct Trigger<S, T>
where
    S: Send + Sync + Clone + 'static,
{
    pub id: Id,
    phantom: PhantomData<T>,
    eve: Eve<S>,
}

impl<S, T> Trigger<S, T>
where
    S: Send + Sync + Clone + 'static,
    T: Send + Sync + Clone + 'static,
{
    pub fn new(eve: Eve<S>) -> Self {
        Self {
            id: Id::new::<T>(),
            phantom: PhantomData,
            eve,
        }
    }

    pub async fn fire(&self) {
        self.eve.mark_subs_as_dirty(self.id).await;
        self.eve.fire_pending_effects().await;
    }
}

#[async_trait]
impl<S> Reactive<S> for TriggerAnchor
where
    S: Send + Sync + Clone + 'static,
{
    async fn run(&self, eve: &Eve<S>) {
        eve.set_node_state(self.id, NodeState::Clean);
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

    pub async fn get(&self) -> Arc<R> {
        self.eve.get_node::<R>().await.unwrap()
    }

    pub async fn set(&self, value: R) {
        let old_value = self.get().await;
        if value != *old_value {
            self.eve
                .set_node_value_by_id(self.id, NodeValue::new(value));
            self.eve.mark_subs_as_dirty(self.id).await;
            self.eve.fire_pending_effects().await;
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
    async fn run(&self, eve: &Eve<S>) {
        let value = (self.handler)();
        eve.set_node_value_by_id(self.id, NodeValue::new(value));
        eve.set_node_state(self.id, NodeState::Clean);
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

    pub async fn get(&self) -> Arc<R> {
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
    async fn run(&self, eve: &Eve<S>) {
        let mut context = EffectContext::new(eve);
        let new_value = self.handler.call(&mut context).await;

        // Update the node value if it's different from the existing one
        if eve
            .get_node_value::<R>()
            .map_or(true, |old_value| new_value != *old_value)
        {
            eve.set_node_value_by_id(self.id, NodeValue::new(new_value));
        }

        // Mark node as clean in either case
        eve.set_node_state(self.id, NodeState::Clean);
    }
}

pub struct Effect<S>
where
    S: Send + Sync + Clone + 'static,
{
    pub id: Id,
    eve: Eve<S>,
}

impl<S> Effect<S>
where
    S: Send + Sync + Clone + 'static,
{
    pub fn new(eve: Eve<S>) -> Self {
        Self {
            id: Id::new::<Self>(),
            eve,
        }
    }

    pub async fn fire(&self) {
        self.eve.set_node_state(self.id, NodeState::Dirty);
        self.eve.fire_pending_effects().await;
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
    async fn run(&self, eve: &Eve<S>) {
        let mut context = EffectContext::new(eve);
        self.handler.call(&mut context).await;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        eve::{Eve, EveBuilder},
        id::Id,
        reactive::Node,
    };

    #[tokio::test]
    async fn test_trigger() {
        let eve_builder = EveBuilder::new(());

        #[derive(Clone, Copy, Debug, Default)]
        struct Trigger;

        async fn effect_fn(_: Node<Trigger>) {
            println!("effect_fn!");
        }

        let eve = eve_builder
            .reg_trigger::<Trigger>()
            .reg_effect(effect_fn)
            .build()
            .unwrap();
        let trigger = eve.get_trigger::<Trigger>().unwrap();
        trigger.fire().await;
    }

    #[tokio::test]
    async fn test_signal() {
        let eve_builder = EveBuilder::new(());

        let eve = eve_builder.reg_signal(|| 7_i32).build().unwrap();
        let signal = eve.get_signal::<i32>().unwrap();
        assert_eq!(*signal.get().await, 7_i32);
        signal.set(8).await;
        assert_eq!(*signal.get().await, 8_i32);
    }

    #[tokio::test]
    async fn test_memo() {
        async fn memo_fn(eve: Eve<()>) -> i32 {
            0
        }
        let eve = EveBuilder::new(()).reg_memo(memo_fn).build().unwrap();
        let memo = eve.get_memo::<i32>().unwrap();
        assert_eq!(*memo.get().await, 0);
    }

    // #[tokio::test]
    // async fn test_effect() {
    //     async fn effect_fn(eve: Eve<()>) {
    //         println!("effect_fn")
    //     }
    //     let eve = EveBuilder::new(()).reg_effect(effect_fn).build().unwrap();
    //     let effect = eve.get_effect::<i32>().unwrap();
    //     assert_eq!(memo.get().await, 0);
    // }

    #[tokio::test]
    async fn test_diamond() {
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeA(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeB(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeC(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeD(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeE(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeF(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeG(i32);
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeH(i32);

        let eve = EveBuilder::new(())
            .reg_signal(|| NodeA(0))
            .reg_memo(|a: Node<NodeA>| async move {
                println!("update B a:{}", a.0 .0);
                NodeB(a.0 .0 + 1)
            })
            .reg_memo(|a: Node<NodeA>| async move {
                println!("update C");
                NodeC(a.0 .0 + 1)
            })
            .reg_memo(|b: Node<NodeB>| async move {
                println!("update D");
                NodeD(b.0 .0 + 1)
            })
            .reg_memo(|b: Node<NodeB>| async move {
                println!("update E");
                NodeE(b.0 .0 + 1)
            })
            .reg_memo(|c: Node<NodeC>| async move {
                println!("update F");
                NodeF(c.0 .0 + 1)
            })
            .reg_memo(|c: Node<NodeC>| async move {
                println!("update G");
                NodeG(c.0 .0 + 1)
            })
            .reg_memo(
                |d: Node<NodeD>, e: Node<NodeE>, f: Node<NodeF>, g: Node<NodeG>| async move {
                    println!("update H");
                    NodeH(d.0 .0 + e.0 .0 + f.0 .0 + g.0 .0)
                },
            )
            .build()
            .unwrap();

        assert_eq!(
            *eve.get_node::<NodeH>().await.unwrap(),
            NodeH(8),
            "Initial NodeH value should be 8"
        );

        println!("set new value to  A");
        eve.set_node_value(NodeA(2)).await;

        assert_eq!(
            *eve.get_node::<NodeB>().await.unwrap(),
            NodeB(3),
            "NodeB value should be 3"
        );
        assert_eq!(
            *eve.get_node::<NodeC>().await.unwrap(),
            NodeC(3),
            "NodeC value should be 3"
        );
        assert_eq!(
            *eve.get_node::<NodeD>().await.unwrap(),
            NodeD(4),
            "NodeD value should be 4"
        );
        assert_eq!(
            *eve.get_node::<NodeE>().await.unwrap(),
            NodeE(4),
            "NodeE value should be 4"
        );
        assert_eq!(
            *eve.get_node::<NodeF>().await.unwrap(),
            NodeF(4),
            "NodeF value should be 4"
        );
        assert_eq!(
            *eve.get_node::<NodeG>().await.unwrap(),
            NodeG(4),
            "NodeG value should be 4"
        );
        assert_eq!(
            *eve.get_node::<NodeH>().await.unwrap(),
            NodeH(16),
            "NodeH value should be 16"
        );
    }

    #[tokio::test]
    async fn test_concurrency() {
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeA;
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeB;
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeC;
        #[derive(Debug, Clone, PartialEq)]
        pub struct NodeD;

        let start_time = std::time::Instant::now();
        let eve = EveBuilder::new(())
            .reg_signal(|| NodeA)
            .reg_memo(|_: Node<NodeA>| async move {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                NodeB
            })
            .reg_memo(|_: Node<NodeA>| async move {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                NodeC
            })
            .reg_memo(|_: Node<NodeB>, _: Node<NodeC>| async move { NodeD })
            .build()
            .unwrap();
        eve.get_node::<NodeD>().await;
        let duration = start_time.elapsed().as_millis();
        assert!(900 < duration && duration < 1100);
    }
}
