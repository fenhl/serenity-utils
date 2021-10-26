//! Provides the [`voice_state_exporter`] function which returns a [`Handler`] that calls [`ExporterMethods`] callbacks when the voice state of a guild changes.

use {
    std::{
        collections::{
            BTreeMap,
            BTreeSet,
        },
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

/// `typemap` key for the voice state data: A mapping of voice channel IDs to their names and users.
#[derive(Default)]
pub struct VoiceStates(pub BTreeMap<ChannelId, (String, Vec<User>)>);

impl TypeMapKey for VoiceStates {
    type Value = VoiceStates;
}

/// Defines callbacks for [`voice_state_exporter`].
pub trait ExporterMethods {
    /// The voice state has changed and should be written to the underlying database.
    fn dump_info<'a>(ctx: &'a Context, guild_id: GuildId, voice_state: &'a VoiceStates) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;

    /// These channels will always be treated as empty. Defaults to the empty set.
    fn ignored_channels<'a>(_: &'a Context) -> Pin<Box<dyn Future<Output = Result<BTreeSet<ChannelId>, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(BTreeSet::default())
        })
    }

    /// Called when the voice channels are no longer empty.
    fn notify_start<'a>(_: &'a Context, _: UserId, _: GuildId, _: ChannelId) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(())
        })
    }
}

/// Calls the given callbacks when the voice state of a guild changes.
pub fn voice_state_exporter<M: ExporterMethods>() -> Handler {
    Handler::default()
        .on_guild_create(false, |ctx, guild, _| Box::pin(async move {
            let VoiceStates(mut chan_map) = VoiceStates::default();
            for (user_id, voice_state) in &guild.voice_states {
                if let Some(channel_id) = voice_state.channel_id {
                    let user = user_id.to_user(&ctx).await?;
                    if chan_map.get(&channel_id).is_none() {
                        chan_map.insert(channel_id, (channel_id.name(&ctx).await.expect("failed to get channel name"), Vec::default()));
                    }
                    let (_, ref mut users) = chan_map.get_mut(&channel_id).expect("just inserted");
                    match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                        Ok(idx) => { users[idx] = user; }
                        Err(idx) => { users.insert(idx, user); }
                    }
                }
            }
            let mut data = ctx.data.write().await;
            data.insert::<VoiceStates>(VoiceStates(chan_map));
            let chan_map = data.get::<VoiceStates>().expect("missing voice states map");
            M::dump_info(ctx, guild.id, chan_map).await?;
            Ok(())
        }))
        .on_voice_state_update(|ctx, guild_id, _, new| Box::pin(async move {
            let guild_id = guild_id.expect("voice_state_update called without guild");
            let user = new.user_id.to_user(&ctx).await?;
            let ignored_channels = M::ignored_channels(ctx).await?;
            let mut data = ctx.data.write().await;
            let voice_states = data.get_mut::<VoiceStates>().expect("missing voice states map");
            let VoiceStates(ref mut chan_map) = voice_states;
            let was_empty = chan_map.iter().all(|(channel_id, (_, members))| members.is_empty() || ignored_channels.contains(channel_id));
            let mut empty_channels = Vec::default();
            for (channel_id, (_, users)) in chan_map.iter_mut() {
                users.retain(|iter_user| iter_user.id != user.id);
                if users.is_empty() {
                    empty_channels.push(*channel_id);
                }
            }
            for channel_id in empty_channels {
                chan_map.remove(&channel_id);
            }
            let chan_id = new.channel_id;
            if let Some(channel_id) = chan_id {
                if chan_map.get(&channel_id).is_none() {
                    chan_map.insert(channel_id, (channel_id.name(&ctx).await.expect("failed to get channel name"), Vec::default()));
                }
                let (_, ref mut users) = chan_map.get_mut(&channel_id).expect("just inserted");
                match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                    Ok(idx) => { users[idx] = user.clone(); }
                    Err(idx) => { users.insert(idx, user.clone()); }
                }
            }
            let is_empty = chan_map.iter().all(|(channel_id, (_, members))| members.is_empty() || ignored_channels.contains(channel_id));
            M::dump_info(ctx, guild_id, voice_states).await?;
            if was_empty && !is_empty {
                M::notify_start(ctx, user.id, guild_id, chan_id.expect("voice channels no longer empty but new channel is None")).await?;
            }
            Ok(())
        }))
}
