use std::{
    any::{type_name, TypeId},
    fmt,
    hash::{Hash, Hasher},
};

use rustc_hash::FxHasher;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Id {
    id: u64,
    get_name: fn() -> String,
}

impl Id {
    pub fn new<T>() -> Self
    where
        T: 'static,
    {
        let type_id = TypeId::of::<T>();
        let mut hasher = FxHasher::default();
        type_id.hash(&mut hasher);
        Self {
            id: hasher.finish(),
            get_name: Self::_name::<T>,
        }
    }

    #[allow(unused_variables)]
    pub fn from<T>(value: &T) -> Self
    where
        T: 'static,
    {
        Id::new::<T>()
    }

    pub fn name(&self) -> String {
        (self.get_name)()
    }

    pub fn _name<T>() -> String {
        type_name::<T>().to_string()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::hash::Hash for Id {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.id);
    }
}
