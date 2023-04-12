#![deny(missing_docs, rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_qualifications, warnings)]

//! This library provides functionality common to multiple [Discord](https://discord.com/) bots maintained by [Fenhl](https://github.com/fenhl).

use {
    std::{
        future::Future,
        sync::Arc,
        time::Duration,
    },
    serenity::{
        gateway::ShardManager,
        model::prelude::*,
        prelude::*,
    },
    tokio::{
        sync::{
            RwLock,
            RwLockMappedWriteGuard,
            RwLockReadGuard,
            RwLockWriteGuard,
        },
        time::sleep,
    },
};
pub use {
    serenity_utils_derive::{
        ipc,
        main,
    },
    crate::builder::Builder,
};
#[doc(hidden)] pub use {
    derive_more,
    futures,
    parking_lot,
    serenity,
    shlex,
    tokio,
    tokio_stream,
}; // used in proc macro

pub mod builder;
pub mod handler;

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
            let _ = tx.send(()); // an error just means no one's listening, which is fine
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
    pub async fn write(&self) -> RwLockMappedWriteGuard<'_, T> {
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

/// Creates a builder for setting up and running a bot.
///
/// An advantage of using this compared to constructing a [`Client`] manually is that the bot will automatically request the required intents.
pub async fn builder(app_id: impl Into<ApplicationId>, token: String) -> serenity::Result<Builder> {
    Builder::new(app_id, token).await
}

/// Utility function to shut down all shards.
pub async fn shut_down(ctx: &Context) {
    ctx.invisible(); // hack to prevent the bot showing as online when it's not
    let data = ctx.data.read().await;
    let mut shard_manager = data.get::<ShardManagerContainer>().expect("missing shard manager").lock().await;
    shard_manager.shutdown_all().await;
    sleep(Duration::from_secs(1)).await; // wait to make sure websockets can be closed cleanly
}
