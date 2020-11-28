#![deny(missing_docs, rust_2018_idioms, unused, unused_import_braces, unused_qualifications, warnings)]

//! This library provides functionality common to multiple [Discord](https://discord.com/) bots maintained by [Fenhl](https://github.com/fenhl).

pub mod handler;
pub mod user_list;
//mod voice_state; //TODO

use {
    std::{
        future::Future,
        sync::Arc,
        time::Duration,
    },
    serenity::{
        client::bridge::gateway::ShardManager,
        prelude::*,
    },
    tokio::{
        sync::{
            RwLock,
            RwLockReadGuard,
            RwLockWriteGuard,
        },
        time::delay_for,
    },
};
pub use serenity_utils_derive::ipc;
/*
pub use crate::{
    user_list::UserListExporter,
    voice_state::VoiceStateExporter,
};
*/ //TODO
#[doc(hidden)] pub use {
    derive_more,
    futures,
    parking_lot,
    serenity,
    shlex,
    tokio,
}; // used in proc macro

#[derive(Debug)]
enum RwFutureData<T: Send + Sync> {
    Pending(tokio::sync::broadcast::Sender<()>),
    Ready(T),
}

impl<T: Send + Sync> RwFutureData<T> {
    fn unwrap(&self) -> &T {
        match self {
            RwFutureData::Pending(_) => panic!("not ready"),
            RwFutureData::Ready(value) => value,
        }
    }

    fn unwrap_mut(&mut self) -> &mut T {
        match self {
            RwFutureData::Pending(_) => panic!("not ready"),
            RwFutureData::Ready(value) => value,
        }
    }
}

/// A type that eventually resolves to its inner type, like a future, but can be accessed like a `RwLock`.
#[derive(Debug, Clone)]
pub struct RwFuture<T: Send + Sync>(Arc<RwLock<RwFutureData<T>>>);

impl<T: Send + Sync + 'static> RwFuture<T> {
    /// Creates a new `RwFuture` which will hold the output of the given future.
    pub fn new<F: Future<Output = T> + Send + 'static>(fut: F) -> RwFuture<T> {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let data = Arc::new(RwLock::new(RwFutureData::Pending(tx.clone())));
        let data_clone = data.clone();
        tokio::spawn(async move {
            let value = fut.await;
            *data_clone.write().await = RwFutureData::Ready(value);
            tx.send(()).expect("failed to notify RwFuture waiters");
        });
        RwFuture(data)
    }

    /// Waits until the value is available, then locks this `RwFuture` for read access.
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        let mut rx = {
            let data = self.0.read().await;
            match *data {
                RwFutureData::Pending(ref tx) => tx.subscribe(),
                RwFutureData::Ready(_) => return RwLockReadGuard::map(data, RwFutureData::unwrap),
            }
        };
        let () = rx.recv().await.expect("RwFuture notifier dropped");
        let data = self.0.read().await;
        match *data {
            RwFutureData::Pending(_) => unreachable!("RwFuture should be ready after notification"),
            RwFutureData::Ready(_) => RwLockReadGuard::map(data, RwFutureData::unwrap),
        }
    }

    /// Waits until the value is available, then locks this `RwFuture` for write access.
    pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
        let mut rx = {
            let data = self.0.write().await;
            match *data {
                RwFutureData::Pending(ref tx) => tx.subscribe(),
                RwFutureData::Ready(_) => return RwLockWriteGuard::map(data, RwFutureData::unwrap_mut),
            }
        };
        let () = rx.recv().await.expect("RwFuture notifier dropped");
        let data = self.0.write().await;
        match *data {
            RwFutureData::Pending(_) => unreachable!("RwFuture should be ready after notification"),
            RwFutureData::Ready(_) => RwLockWriteGuard::map(data, RwFutureData::unwrap_mut),
        }
    }
}

impl<T: Send + Sync + Default> Default for RwFuture<T> {
    fn default() -> RwFuture<T> {
        RwFuture(Arc::new(RwLock::new(RwFutureData::Ready(T::default()))))
    }
}

/// A `typemap` key holding the [`ShardManager`]. Used in `shut_down`.
pub struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}

/// Utility function to shut down all shards.
pub async fn shut_down(ctx: &Context) {
    ctx.invisible().await; // hack to prevent the bot showing as online when it's not
    let data = ctx.data.read().await;
    let mut shard_manager = data.get::<ShardManagerContainer>().expect("missing shard manager").lock().await;
    shard_manager.shutdown_all().await;
    delay_for(Duration::from_secs(1)).await; // wait to make sure websockets can be closed cleanly
}
