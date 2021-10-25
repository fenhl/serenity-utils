//! Provides the [`voice_state_exporter`] function which returns a [`Handler`] that calls [`ExporterMethods`] callbacks when the voice state of a guild changes.

use {
    std::{
        collections::BTreeMap,
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

/// `typemap` key for the voice state data: A mapping of voice channel names to users.
pub struct VoiceStates;

impl TypeMapKey for VoiceStates {
    type Value = BTreeMap<String, Vec<User>>;
}

/// Defines callbacks for [`voice_state_exporter`].
pub trait ExporterMethods {
    /// The voice state has changed and should be written to the underlying database.
    fn dump_info<'a>(ctx: &'a Context, voice_state: &'a <VoiceStates as TypeMapKey>::Value) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;
}

/// Calls the given callbacks when the voice state of a guild changes.
pub fn voice_state_exporter<M: ExporterMethods>() -> Handler {
    Handler::default()
        .on_guild_create(false, |ctx, guild, _| Box::pin(async move {
            let mut chan_map = <VoiceStates as TypeMapKey>::Value::default();
            for (user_id, voice_state) in &guild.voice_states {
                if let Some(channel_id) = voice_state.channel_id {
                    let user = user_id.to_user(&ctx).await?;
                    let users = chan_map.entry(channel_id.name(&ctx).await.expect("failed to get channel name")).or_default();
                    match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                        Ok(idx) => { users[idx] = user; }
                        Err(idx) => { users.insert(idx, user); }
                    }
                }
            }
            let mut data = ctx.data.write().await;
            data.insert::<VoiceStates>(chan_map);
            let chan_map = data.get::<VoiceStates>().expect("missing voice states map");
            M::dump_info(ctx, chan_map).await?;
            Ok(())
        }))
        .on_voice_state_update(|ctx, _, _, new| Box::pin(async move {
            let user = new.user_id.to_user(&ctx).await?;
            let mut data = ctx.data.write().await;
            let chan_map = data.get_mut::<VoiceStates>().expect("missing voice states map");
            let mut empty_channels = Vec::default();
            for (channel_name, users) in chan_map.iter_mut() {
                users.retain(|iter_user| iter_user.id != user.id);
                if users.is_empty() {
                    empty_channels.push(channel_name.to_owned());
                }
            }
            for channel_name in empty_channels {
                chan_map.remove(&channel_name);
            }
            if let Some(channel_id) = new.channel_id {
                let users = chan_map.entry(channel_id.name(&ctx).await.expect("failed to get channel name")).or_default();
                match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                    Ok(idx) => { users[idx] = user; }
                    Err(idx) => { users.insert(idx, user); }
                }
            }
            M::dump_info(ctx, chan_map).await?;
            Ok(())
        }))
}
