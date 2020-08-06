//! Extensions for serenity's `EventHandler` trait.

use serenity::prelude::*;

/// A trait that can be used to have multiple `EventHandlers` in serenity. This is accomplished using the `EventHandler` implementation for `HandlerList`.
///
/// See `serenity::prelude::EventHandler` for method documentation.
pub trait EventHandlerRef {}

/// A wrapper type that implements `serenity::prelude::EventHandler` by calling each element in order.
pub struct HandlerList(pub Vec<Box<dyn EventHandlerRef + Send + Sync>>);

impl EventHandler for HandlerList {
    //TODO make sure all methods defined in EventHandlerRef are actually called here
}
