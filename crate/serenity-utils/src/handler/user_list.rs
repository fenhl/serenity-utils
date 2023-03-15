//! Provides the [`user_list_exporter`] function which returns a [`Handler`] that calls [`ExporterMethods`] callbacks when the member list of a guild changes.

use {
    std::{
        future::Future,
        pin::Pin,
    },
    serenity::{
        model::prelude::*,
        prelude::*,
    },
    super::{
        Handler,
        HandlerMethods as _,
    },
};

#[derive(Debug, thiserror::Error)]
#[error("received guild member update event for uncached member")]
struct GuildMemberUpdateError;

/// Defines callbacks for [`user_list_exporter`].
pub trait ExporterMethods {
    /// A member has been added or modified and its record should be inserted into or updated in the underlying database.
    fn upsert<'a>(ctx: &'a Context, member: &'a Member) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;
    /// All member records in the underlying database should be replaced with the given ones.
    fn replace_all<'a>(ctx: &'a Context, members: Vec<&'a Member>) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;
    /// The member record for the given user in the given guild should be deleted, if it exists.
    fn remove<'a>(ctx: &'a Context, user_id: UserId, guild_id: GuildId) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;
}

/// Calls the given callbacks when the member list of a guild changes.
pub fn user_list_exporter<M: ExporterMethods>() -> Handler {
    Handler::default()
        .on_guild_ban_addition(|ctx, guild_id, user| M::remove(ctx, user.id, guild_id))
        .on_guild_ban_removal(|ctx, guild_id, user| Box::pin(async move {
            M::upsert(ctx, &guild_id.member(ctx, user).await?).await
        }))
        .on_guild_create(true, |ctx, guild, _| M::replace_all(ctx, guild.members.values().collect()))
        .on_guild_member_addition(|ctx, member| M::upsert(ctx, member))
        .on_guild_member_removal(|ctx, guild_id, user, _| M::remove(ctx, user.id, guild_id))
        .on_guild_member_update(|ctx, _, member, _| Box::pin(async move { M::upsert(ctx, member.ok_or(GuildMemberUpdateError)?).await }))
        .on_guild_members_chunk(|ctx, chunk| Box::pin(async move {
            for member in chunk.members.values() {
                M::upsert(ctx, member).await?;
            }
            Ok(())
        }))
}
