#![allow(clippy::type_complexity)]
#![allow(clippy::new_without_default)]

use std::fmt;

use downcast_rs::DowncastSync;
use dyn_clone::DynClone;
pub mod effect_handler;
pub mod error;
pub mod eve;
pub mod event;
pub mod event_handler;
pub mod id;
pub mod reactive;

pub trait BoxableValue: DowncastSync + DynClone + fmt::Debug {}
dyn_clone::clone_trait_object!(BoxableValue);
downcast_rs::impl_downcast!(sync BoxableValue);
impl<T: Send + Sync + Clone + fmt::Debug + 'static> BoxableValue for T {}
