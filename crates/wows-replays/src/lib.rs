#![allow(clippy::result_large_err)]
#![allow(clippy::mutable_key_type)]
#![allow(clippy::arc_with_non_send_sync)]
#![allow(clippy::module_inception)]

pub mod analyzer;
#[cfg(feature = "parsing")]
mod error;
#[cfg(feature = "parsing")]
pub mod game_constants;
#[cfg(feature = "parsing")]
pub mod nested_property_path;
#[cfg(feature = "parsing")]
pub mod packet2;
pub mod types;
#[cfg(feature = "parsing")]
mod wowsreplay;

#[cfg(feature = "parsing")]
pub use error::*;
#[cfg(feature = "parsing")]
pub use wowsreplay::*;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;
#[cfg(feature = "arc")]
pub type RwCell<T> = std::sync::RwLock<T>;
#[cfg(feature = "arc")]
pub type RwCellReadGuard<'a, T> = std::sync::RwLockReadGuard<'a, T>;
#[cfg(feature = "arc")]
pub type RwCellWriteGuard<'a, T> = std::sync::RwLockWriteGuard<'a, T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
#[cfg(not(feature = "arc"))]
pub type RwCell<T> = std::cell::RefCell<T>;
#[cfg(not(feature = "arc"))]
pub type RwCellReadGuard<'a, T> = std::cell::Ref<'a, T>;
#[cfg(not(feature = "arc"))]
pub type RwCellWriteGuard<'a, T> = std::cell::RefMut<'a, T>;

/// Uniform access to interior-mutable cells (`RefCell` or `RwLock`).
pub trait RwCellExt<T> {
    fn read_ref(&self) -> RwCellReadGuard<'_, T>;
    fn write_ref(&self) -> RwCellWriteGuard<'_, T>;
}

#[cfg(feature = "arc")]
impl<T> RwCellExt<T> for std::sync::RwLock<T> {
    fn read_ref(&self) -> RwCellReadGuard<'_, T> {
        self.read().expect("RwLock poisoned")
    }
    fn write_ref(&self) -> RwCellWriteGuard<'_, T> {
        self.write().expect("RwLock poisoned")
    }
}

#[cfg(not(feature = "arc"))]
impl<T> RwCellExt<T> for std::cell::RefCell<T> {
    fn read_ref(&self) -> RwCellReadGuard<'_, T> {
        self.borrow()
    }
    fn write_ref(&self) -> RwCellWriteGuard<'_, T> {
        self.borrow_mut()
    }
}
