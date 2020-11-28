//! Extensions for serenity's [`EventHandler`] trait.

use {
    std::sync::Arc,
    async_trait::async_trait,
    serenity::{
        model::prelude::*,
        prelude::*,
    },
};

/*
/// A trait that can be used to have multiple [`EventHandler`]s in serenity. This is accomplished using the [`EventHandler`] implementation for [`HandlerList`].
///
/// See [`serenity::prelude::EventHandler`] for method documentation.
#[async_trait]
pub trait EventHandlerRef {
    async fn guild_ban_addition(&self, ctx: &Context, guild_id: &GuildId, banned_user: &User);
}
*/

/// A wrapper type that implements [`serenity::prelude::EventHandler`] by calling each element in parallel.
pub struct HandlerList(pub Vec<Arc<dyn EventHandler>>);

#[async_trait]
impl EventHandler for HandlerList {
    //TODO make sure all methods defined in EventHandler are actually called here

    async fn guild_ban_addition(&self, ctx: Context, guild_id: GuildId, banned_user: User) {
        for handler in &self.0 {
            let handler = Arc::clone(handler);
            let ctx = ctx.clone();
            let banned_user = banned_user.clone();
            tokio::spawn(async move {
                handler.guild_ban_addition(ctx, guild_id, banned_user).await
            });
        }
    }
}
