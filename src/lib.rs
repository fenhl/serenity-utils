//! This library provides wrapper implementations of `serenity::EventHandler`.

#![cfg_attr(test, deny(warnings))]
#![warn(trivial_casts)]
#![deny(unused, missing_docs, unused_qualifications)]
#![forbid(unused_import_braces)]

#[macro_use] extern crate serde_json;
extern crate serenity;
extern crate typemap;

mod user_list;
mod voice_state;

pub use user_list::UserListExporter;
pub use voice_state::VoiceStateExporter;
