use {
    std::{
        collections::BTreeMap,
        fs::File,
        io,
        path::PathBuf,
    },
    async_trait::async_trait,
    serde_json::json,
    serenity::{
        model::prelude::*,
        prelude::*,
    },
    typemap::Key,
    crate::handler::EventHandlerRef,
};

/// `typemap` key for the voice state data to be serialized.
pub struct VoiceStates;

impl Key for VoiceStates {
    type Value = BTreeMap<String, Vec<User>>;
}

/// An `EventHandler` which writes a JSON representation of the current voice channel states (i.e. who's in them) to a given path.
pub struct VoiceStateExporter {
    path: PathBuf
}

impl VoiceStateExporter {
    /// Returns a new `VoiceStateExporter` which writes to the given path.
    pub fn new(path: impl Into<PathBuf>) -> VoiceStateExporter {
        VoiceStateExporter {
            path: path.into()
        }
    }

    fn dump_info(&self, voice_states: &<VoiceStates as Key>::Value) -> io::Result<()> {
        let f = File::create(&self.path)?;
        serde_json::to_writer(f, &json!({
            "channels": voice_states.into_iter()
                .map(|(channel_name, members)| json!({
                    "members": members.into_iter()
                        .map(|user| json!({
                            "discriminator": user.discriminator,
                            "snowflake": user.id,
                            "username": user.name
                        }))
                        .collect::<Vec<_>>(),
                    "name": channel_name
                }))
                .collect::<Vec<_>>()
        }))?;
        Ok(())
    }
}

#[async_trait]
impl EventHandlerRef for VoiceStateExporter {
    async fn guild_create(&self, ctx: Context, guild: Guild, _: bool) {
        let mut chan_map = <VoiceStates as Key>::Value::default();
        for (user_id, voice_state) in guild.voice_states {
            if let Some(channel_id) = voice_state.channel_id {
                let user = user_id.to_user().expect("failed to get user info");
                let users = chan_map.entry(channel_id.name().expect("failed to get channel name"))
                    .or_insert_with(Vec::default);
                match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                    Ok(idx) => { users[idx] = user; }
                    Err(idx) => { users.insert(idx, user); }
                }
            }
        }
        let mut data = ctx.data.write().await;
        data.insert::<VoiceStates>(chan_map);
        let chan_map = data.get::<VoiceStates>().expect("missing voice states map");
        self.dump_info(chan_map).expect("failed to dump voice state");
    }

    async fn voice_state_update(&self, ctx: Context, _: Option<GuildId>, voice_state: VoiceState) {
        let user = voice_state.user_id.to_user().expect("failed to get user info");
        let mut data = ctx.data.write();
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
        if let Some(channel_id) = voice_state.channel_id {
            let users = chan_map.entry(channel_id.name(&ctx).await.expect("failed to get channel name"))
                .or_insert_with(Vec::default);
            match users.binary_search_by_key(&(user.name.clone(), user.discriminator), |user| (user.name.clone(), user.discriminator)) {
                Ok(idx) => { users[idx] = user; }
                Err(idx) => { users.insert(idx, user); }
            }
        }
        self.dump_info(chan_map).expect("failed to dump voice state");
    }
}
