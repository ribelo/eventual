use crate::{eve::Eve, event::Event, side_effect::SideEffect};

pub enum Effect<T> {
    Event(Box<dyn Event<Eve = T>>),
    SideEffects(Box<dyn SideEffect<Eve = T>>),
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
    pub fn add_effect(&mut self, effect: Effect<T>) {
        self.effects.push(effect);
    }

    pub fn add_effects(&mut self, effects: impl IntoIterator<Item = Effect<T>>) {
        self.effects.extend(effects);
    }

    pub fn extend(&mut self, other: Self) {
        self.effects.extend(other.effects);
    }
}

impl<T> From<Effect<T>> for Effects<T> {
    fn from(effect: Effect<T>) -> Self {
        Self {
            effects: vec![effect],
        }
    }
}

impl<T> From<Box<dyn Event<Eve = T>>> for Effect<T> {
    fn from(event: Box<dyn Event<Eve = T>>) -> Self {
        Self::Event(event)
    }
}

impl<T> From<Box<dyn Event<Eve = T>>> for Effects<T> {
    fn from(event: Box<dyn Event<Eve = T>>) -> Self {
        Self {
            effects: vec![Effect::Event(event)],
        }
    }
}
