use crate::{event::Event, side_effect::SideEffect};

pub enum Effect<T> {
    Event(Box<dyn Event<T>>),
    SideEffects(Box<dyn SideEffect<T>>),
}

pub struct Effects<T> {
    pub(crate) effects: Vec<Effect<T>>,
}

impl<T> Default for Effects<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Effects<T> {
    pub fn new() -> Self {
        Self { effects: vec![] }
    }

    pub fn none() -> Self {
        Self::new()
    }

    pub fn add_event(mut self, event: impl Event<T> + 'static) -> Self {
        self.effects.push(Effect::Event(Box::new(event)));
        self
    }

    pub fn add_side_effect(mut self, side_effect: impl SideEffect<T> + 'static) -> Self {
        self.effects
            .push(Effect::SideEffects(Box::new(side_effect)));
        self
    }

    pub fn add_effect(mut self, effect: Effect<T>) -> Self {
        self.effects.push(effect);
        self
    }

    pub fn add_effects(mut self, effects: impl IntoIterator<Item = Effect<T>>) -> Self {
        self.effects.extend(effects);
        self
    }

    pub fn extend(mut self, other: Self) -> Self {
        self.effects.extend(other.effects);
        self
    }
}

impl<T> From<Effect<T>> for Effects<T> {
    fn from(effect: Effect<T>) -> Self {
        Self {
            effects: vec![effect],
        }
    }
}

impl<T> From<Box<dyn Event<T>>> for Effect<T> {
    fn from(event: Box<dyn Event<T>>) -> Self {
        Self::Event(event)
    }
}

impl<T> From<Box<dyn Event<T>>> for Effects<T> {
    fn from(event: Box<dyn Event<T>>) -> Self {
        Self {
            effects: vec![Effect::Event(event)],
        }
    }
}
