//! Extensions for serenity's [`EventHandler`] trait.

use {
    std::{
        future::Future,
        pin::Pin,
        sync::Arc,
    },
    serenity::{
        all::Interaction,
        model::prelude::*,
        prelude::*,
    },
    tokio::sync::Mutex,
    crate::{
        builder::ErrorNotifier,
        shut_down,
    },
};
pub use self::{
    user_list::user_list_exporter,
    voice_state::voice_state_exporter,
};

pub mod user_list;
pub mod voice_state;

pub(crate) type Output<'r> = Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'r>>;

#[allow(missing_docs)] //TODO link to equivalent methods on serenity?
pub trait HandlerMethods {
    fn on_ready(self, f: for<'r> fn(&'r Context, &'r Ready) -> Output<'r>) -> Self;
    fn on_guild_ban_addition(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self;
    fn on_guild_ban_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self;
    fn on_guild_create(self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, Option<bool>) -> Output<'r>) -> Self;
    fn on_guild_member_addition(self, f: for<'r> fn(&'r Context, &'r Member) -> Output<'r>) -> Self;
    fn on_guild_member_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>) -> Self;
    fn on_guild_member_update(self, f: for<'r> fn(&'r Context, Option<&'r Member>, Option<&'r Member>, &'r GuildMemberUpdateEvent) -> Output<'r>) -> Self;
    fn on_guild_members_chunk(self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>) -> Self;
    fn on_interaction_create(self, f: for<'r> fn(&'r Context, &'r Interaction) -> Output<'r>) -> Self;
    fn on_guild_role_create(self, f: for<'r> fn(&'r Context, &'r Role) -> Output<'r>) -> Self;
    fn on_message(self, require_content: bool, f: for<'r> fn(&'r Context, &'r Message) -> Output<'r>) -> Self;
    fn on_voice_state_update(self, f: for<'r> fn(&'r Context, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>) -> Self;
}

/// A type that implements serenity's [`EventHandler`](serenity::client::EventHandler) trait, but with a more convenient interface, such as requesting intents automatically.
///
/// Use the trait methods on [`HandlerMethods`] to configure this handler.
#[derive(Default)]
pub struct Handler {
    pub(crate) ctx_tx: Option<Arc<Mutex<Option<tokio::sync::oneshot::Sender<Context>>>>>,
    pub(crate) intents: GatewayIntents,
    ready: Vec<for<'r> fn(&'r Context, &'r Ready) -> Output<'r>>,
    guild_ban_addition: Vec<for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>>,
    guild_ban_removal: Vec<for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>>,
    guild_create: Vec<for<'r> fn(&'r Context, &'r Guild, Option<bool>) -> Output<'r>>,
    guild_member_addition: Vec<for<'r> fn(&'r Context, &'r Member) -> Output<'r>>,
    guild_member_removal: Vec<for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>>,
    guild_member_update: Vec<for<'r> fn(&'r Context, Option<&'r Member>, Option<&'r Member>, &'r GuildMemberUpdateEvent) -> Output<'r>>,
    guild_members_chunk: Vec<for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>>,
    interaction_create: Vec<for<'r> fn(&'r Context, &'r Interaction) -> Output<'r>>,
    guild_role_create: Vec<for<'r> fn(&'r Context, &'r Role) -> Output<'r>>,
    message: Vec<for<'r> fn(&'r Context, &'r Message) -> Output<'r>>,
    voice_state_update: Vec<for<'r> fn(&'r Context, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>>,
}

impl Handler {
    pub(crate) fn merge(&mut self, other: Self) {
        let Handler { ctx_tx, intents, ready, guild_ban_addition, guild_ban_removal, guild_create, guild_member_addition, guild_member_removal, guild_member_update, guild_members_chunk, interaction_create, guild_role_create, message, voice_state_update } = other;
        if let Some(ctx_tx) = ctx_tx {
            self.ctx_tx.get_or_insert(ctx_tx);
        }
        self.intents |= intents;
        self.ready.extend(ready);
        self.guild_ban_addition.extend(guild_ban_addition);
        self.guild_ban_removal.extend(guild_ban_removal);
        self.guild_create.extend(guild_create);
        self.guild_member_addition.extend(guild_member_addition);
        self.guild_member_removal.extend(guild_member_removal);
        self.guild_member_update.extend(guild_member_update);
        self.guild_members_chunk.extend(guild_members_chunk);
        self.interaction_create.extend(interaction_create);
        self.guild_role_create.extend(guild_role_create);
        self.message.extend(message);
        self.voice_state_update.extend(voice_state_update);
    }
}

impl HandlerMethods for Handler {
    fn on_ready(mut self, f: for<'r> fn(&'r Context, &'r Ready) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'r>>) -> Self {
        self.ready.push(f);
        self
    }

    fn on_guild_ban_addition(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_BANS;
        self.guild_ban_addition.push(f);
        self
    }

    fn on_guild_ban_removal(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_BANS;
        self.guild_ban_removal.push(f);
        self
    }

    fn on_guild_create(mut self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, Option<bool>) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILDS;
        if require_members { self.intents |= GatewayIntents::GUILD_PRESENCES }
        self.guild_create.push(f);
        self
    }

    fn on_guild_member_addition(mut self, f: for<'r> fn(&'r Context, &'r Member) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_addition.push(f);
        self
    }

    fn on_guild_member_removal(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_removal.push(f);
        self
    }

    fn on_guild_member_update(mut self, f: for<'r> fn(&'r Context, Option<&'r Member>, Option<&'r Member>, &'r GuildMemberUpdateEvent) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_update.push(f);
        self
    }

    fn on_guild_members_chunk(mut self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>) -> Self {
        self.guild_members_chunk.push(f);
        self
    }

    fn on_interaction_create(mut self, f: for<'r> fn(&'r Context, &'r Interaction) -> Output<'r>) -> Self {
        self.interaction_create.push(f);
        self
    }

    fn on_guild_role_create(mut self, f: for<'r> fn(&'r Context, &'r Role) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILDS;
        self.guild_role_create.push(f);
        self
    }

    fn on_message(mut self, require_content: bool, f: for<'r> fn(&'r Context, &'r Message) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES; //TODO allow customizing which to receive?
        if require_content { self.intents |= GatewayIntents::MESSAGE_CONTENT }
        self.message.push(f);
        self
    }

    fn on_voice_state_update(mut self, f: for<'r> fn(&'r Context, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_VOICE_STATES;
        self.voice_state_update.push(f);
        self
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, data_about_bot: Ready) {
        if let Some(ref tx) = self.ctx_tx {
            if let Some(tx) = tx.lock().await.take() {
                if let Err(_) = tx.send(ctx.clone()) {
                    panic!("failed to send context")
                }
            }
        }
        let guilds = data_about_bot.user.guilds(&ctx).await.expect("failed to get guilds");
        if guilds.is_empty() {
            println!("No guilds found, use following URL to invite the bot:");
            println!("{}", data_about_bot.user.invite_url(&ctx, Permissions::all()).await.expect("failed to generate invite URL")); //TODO allow customizing permissions?
            shut_down(&ctx).await; //TODO allow running without guilds?
        }
        for f in &self.ready {
            if let Err(e) = f(&ctx, &data_about_bot).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `ready` event", e).await;
                }
            }
        }
    }

    async fn guild_ban_addition(&self, ctx: Context, guild_id: GuildId, banned_user: User) {
        for f in &self.guild_ban_addition {
            if let Err(e) = f(&ctx, guild_id, &banned_user).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_ban_addition` event", e).await;
                }
            }
        }
    }

    async fn guild_ban_removal(&self, ctx: Context, guild_id: GuildId, unbanned_user: User) {
        for f in &self.guild_ban_removal {
            if let Err(e) = f(&ctx, guild_id, &unbanned_user).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_ban_removal` event", e).await;
                }
            }
        }
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        for f in &self.guild_create {
            if let Err(e) = f(&ctx, &guild, is_new).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_create` event", e).await;
                }
            }
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        for f in &self.guild_member_addition {
            if let Err(e) = f(&ctx, &new_member).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_member_addition` event", e).await;
                }
            }
        }
    }

    async fn guild_member_removal(&self, ctx: Context, guild_id: GuildId, user: User, member_data_if_available: Option<Member>) {
        for f in &self.guild_member_removal {
            if let Err(e) = f(&ctx, guild_id, &user, member_data_if_available.as_ref()).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_member_removal` event", e).await;
                }
            }
        }
    }

    async fn guild_member_update(&self, ctx: Context, old_if_available: Option<Member>, new: Option<Member>, event: GuildMemberUpdateEvent) {
        for f in &self.guild_member_update {
            if let Err(e) = f(&ctx, old_if_available.as_ref(), new.as_ref(), &event).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_member_update` event", e).await;
                }
            }
        }
    }

    async fn guild_members_chunk(&self, ctx: Context, chunk: GuildMembersChunkEvent) {
        for f in &self.guild_members_chunk {
            if let Err(e) = f(&ctx, &chunk).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `guild_members_chunk` event", e).await;
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        for f in &self.interaction_create {
            if let Err(e) = f(&ctx, &interaction).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `interaction_create` event", e).await;
                }
            }
        }
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        for f in &self.message {
            if let Err(e) = f(&ctx, &new_message).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `message` event", e).await;
                }
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        for f in &self.voice_state_update {
            if let Err(e) = f(&ctx, old.as_ref(), &new).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, "error in `voice_state_update` event", e).await;
                }
            }
        }
    }
}
