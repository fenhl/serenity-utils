//! Extensions for serenity's [`EventHandler`] trait.

use {
    std::{
        future::Future,
        pin::Pin,
        sync::Arc,
    },
    serenity::{
        builder::CreateApplicationCommandsPermissions,
        client::bridge::gateway::GatewayIntents,
        model::{
            interactions::application_command::ApplicationCommandPermissionType,
            prelude::*,
        },
        prelude::*,
    },
    tokio::sync::Mutex,
    crate::{
        RwFuture,
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
    /// Adds a slash command.
    ///
    /// The command will be automatically created or updated each time the bot connects to Discord. Note that commands not specified will *not* be deleted.
    fn slash_command(self, cmd: crate::slash::Command) -> Self;

    fn on_ready(self, f: for<'r> fn(&'r Context, &'r Ready) -> Output<'r>) -> Self;
    fn on_guild_ban_addition(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self;
    fn on_guild_ban_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>) -> Self;
    fn on_guild_create(self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, bool) -> Output<'r>) -> Self;
    fn on_guild_member_addition(self, f: for<'r> fn(&'r Context, GuildId, &'r Member) -> Output<'r>) -> Self;
    fn on_guild_member_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>) -> Self;
    fn on_guild_member_update(self, f: for<'r> fn(&'r Context, Option<&'r Member>, &'r Member) -> Output<'r>) -> Self;
    fn on_guild_members_chunk(self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>) -> Self;
    fn on_message(self, f: for<'r> fn(&'r Context, &'r Message) -> Output<'r>) -> Self;
    fn on_voice_state_update(self, f: for<'r> fn(&'r Context, Option<GuildId>, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>) -> Self;
}

/// A type that implements serenity's [`EventHandler`](serenity::client::EventHandler) trait, but with a more convenient interface, such as requesting intents automatically.
///
/// Use the trait methods on [`HandlerMethods`] to configure this handler.
#[derive(Default)]
pub struct Handler {
    ctx_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Context>>>>,
    slash_commands: Vec<crate::slash::Command>,
    pub(crate) intents: GatewayIntents,
    ready: Vec<for<'r> fn(&'r Context, &'r Ready) -> Output<'r>>,
    guild_ban_addition: Vec<for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>>,
    guild_ban_removal: Vec<for<'r> fn(&'r Context, GuildId, &'r User) -> Output<'r>>,
    guild_create: Vec<for<'r> fn(&'r Context, &'r Guild, bool) -> Output<'r>>,
    guild_member_addition: Vec<for<'r> fn(&'r Context, GuildId, &'r Member) -> Output<'r>>,
    guild_member_removal: Vec<for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>>,
    guild_member_update: Vec<for<'r> fn(&'r Context, Option<&'r Member>, &'r Member) -> Output<'r>>,
    guild_members_chunk: Vec<for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>>,
    message: Vec<for<'r> fn(&'r Context, &'r Message) -> Output<'r>>,
    voice_state_update: Vec<for<'r> fn(&'r Context, Option<GuildId>, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>>,
}

impl Handler {
    pub(crate) fn new_with_ctx() -> (Self, RwFuture<Context>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Self {
                ctx_tx: Arc::new(Mutex::new(Some(tx))),
                ..Self::default()
            },
            RwFuture::new(async move { rx.await.expect("failed to store handler context") }),
        )
    }

    pub(crate) fn merge(&mut self, other: Self) {
        let Handler { ctx_tx: _, slash_commands, intents, ready, guild_ban_addition, guild_ban_removal, guild_create, guild_member_addition, guild_member_removal, guild_member_update, guild_members_chunk, message, voice_state_update } = other;
        self.slash_commands.extend(slash_commands);
        self.intents |= intents;
        self.ready.extend(ready);
        self.guild_ban_addition.extend(guild_ban_addition);
        self.guild_ban_removal.extend(guild_ban_removal);
        self.guild_create.extend(guild_create);
        self.guild_member_addition.extend(guild_member_addition);
        self.guild_member_removal.extend(guild_member_removal);
        self.guild_member_update.extend(guild_member_update);
        self.guild_members_chunk.extend(guild_members_chunk);
        self.message.extend(message);
        self.voice_state_update.extend(voice_state_update);
    }

    async fn setup_slash_commands(&self, ctx: &Context, guild_id: GuildId) -> serenity::Result<()> {
        let existing_commands = guild_id.get_application_commands(ctx).await?;
        let mut all_perms = CreateApplicationCommandsPermissions::default();
        for cmd in &self.slash_commands {
            if cmd.guild_id == guild_id {
                let cmd_id = if let Some(existing_command) = existing_commands.iter().find(|iter_cmd| iter_cmd.name == cmd.name) {
                    //TODO only update if changed
                    guild_id.edit_application_command(ctx, existing_command.id, |setup| { *setup = cmd.setup.clone(); setup }).await?;
                    existing_command.id
                } else {
                    guild_id.create_application_command(ctx, |setup| { *setup = cmd.setup.clone(); setup }).await?.id
                };
                all_perms.create_application_command(|cmd_perms| {
                    cmd_perms.id(cmd_id.0);
                    for role in &cmd.perms.roles { cmd_perms.create_permissions(|p| p.kind(ApplicationCommandPermissionType::Role).id(role.0).permission(true)); }
                    for user in &cmd.perms.users { cmd_perms.create_permissions(|p| p.kind(ApplicationCommandPermissionType::User).id(user.0).permission(true)); }
                    cmd_perms
                });
            }
        }
        guild_id.set_application_commands_permissions(ctx, |p| { *p = all_perms; p }).await?;
        Ok(())
    }
}

impl HandlerMethods for Handler {
    fn slash_command(mut self, cmd: crate::slash::Command) -> Self {
        self.slash_commands.push(cmd);
        self
    }

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

    fn on_guild_create(mut self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, bool) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILDS;
        if require_members { self.intents |= GatewayIntents::GUILD_PRESENCES }
        self.guild_create.push(f);
        self
    }

    fn on_guild_member_addition(mut self, f: for<'r> fn(&'r Context, GuildId, &'r Member) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_addition.push(f);
        self
    }

    fn on_guild_member_removal(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_removal.push(f);
        self
    }

    fn on_guild_member_update(mut self, f: for<'r> fn(&'r Context, Option<&'r Member>, &'r Member) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MEMBERS;
        self.guild_member_update.push(f);
        self
    }

    fn on_guild_members_chunk(mut self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> Output<'r>) -> Self {
        self.guild_members_chunk.push(f);
        self
    }

    fn on_message(mut self, f: for<'r> fn(&'r Context, &'r Message) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES; //TODO allow customizing which to receive?
        self.message.push(f);
        self
    }

    fn on_voice_state_update(mut self, f: for<'r> fn(&'r Context, Option<GuildId>, Option<&'r VoiceState>, &'r VoiceState) -> Output<'r>) -> Self {
        self.intents |= GatewayIntents::GUILD_VOICE_STATES;
        self.voice_state_update.push(f);
        self
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, data_about_bot: Ready) {
        if let Some(tx) = self.ctx_tx.lock().await.take() {
            if let Err(_) = tx.send(ctx.clone()) {
                panic!("failed to send context")
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
                    let _ = error_notifier.say(&ctx, format!("error in `ready` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_ban_addition(&self, ctx: Context, guild_id: GuildId, banned_user: User) {
        for f in &self.guild_ban_addition {
            if let Err(e) = f(&ctx, guild_id, &banned_user).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_ban_addition` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_ban_removal(&self, ctx: Context, guild_id: GuildId, unbanned_user: User) {
        for f in &self.guild_ban_removal {
            if let Err(e) = f(&ctx, guild_id, &unbanned_user).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_ban_removal` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        if let Err(e) = self.setup_slash_commands(&ctx, guild.id).await {
            if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                let _ = error_notifier.say(&ctx, format!("error setting up slash commands: `{:?}`", e)).await;
            }
        }
        for f in &self.guild_create {
            if let Err(e) = f(&ctx, &guild, is_new).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_create` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_member_addition(&self, ctx: Context, guild_id: GuildId, new_member: Member) {
        for f in &self.guild_member_addition {
            if let Err(e) = f(&ctx, guild_id, &new_member).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_member_addition` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_member_removal(&self, ctx: Context, guild_id: GuildId, user: User, member_data_if_available: Option<Member>) {
        for f in &self.guild_member_removal {
            if let Err(e) = f(&ctx, guild_id, &user, member_data_if_available.as_ref()).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_member_removal` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_member_update(&self, ctx: Context, old_if_available: Option<Member>, new: Member) {
        for f in &self.guild_member_update {
            if let Err(e) = f(&ctx, old_if_available.as_ref(), &new).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_member_update` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn guild_members_chunk(&self, ctx: Context, chunk: GuildMembersChunkEvent) {
        for f in &self.guild_members_chunk {
            if let Err(e) = f(&ctx, &chunk).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `guild_members_chunk` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(interaction) = interaction {
            if let Some(guild_id) = interaction.guild_id {
                for cmd in &self.slash_commands {
                    if cmd.guild_id == guild_id && cmd.name == interaction.data.name {
                        if let Err(e) = (cmd.handle)(&ctx, interaction).await {
                            if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                                let _ = error_notifier.say(&ctx, format!("error in handler for /{}: `{:?}`", cmd.name, e)).await;
                            }
                        }
                        break
                    }
                }
            }
        }
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        for f in &self.message {
            if let Err(e) = f(&ctx, &new_message).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `message` event: `{:?}`", e)).await;
                }
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, arg2: Option<GuildId>, old: Option<VoiceState>, new: VoiceState) {
        for f in &self.voice_state_update {
            if let Err(e) = f(&ctx, arg2, old.as_ref(), &new).await {
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    let _ = error_notifier.say(&ctx, format!("error in `voice_state_update` event: `{:?}`", e)).await;
                }
            }
        }
    }
}
