#![deny(missing_docs, rust_2018_idioms, unused, unused_import_braces, unused_qualifications, warnings)]

//! This library provides wrapper implementations of `serenity::EventHandler`.

pub mod handler;

pub use serenity_utils_derive::ipc;

#[doc(hidden)] pub use {
    derive_more,
    parking_lot,
    serenity,
    shlex
}; // used in proc macro

/*
mod user_list;
mod voice_state;

pub use user_list::UserListExporter;
pub use voice_state::VoiceStateExporter;
*/ //TODO migrate to EventHandlerRef system and uncomment
