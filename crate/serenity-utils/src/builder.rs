//! Contains the [`Builder`] type returned by [`serenity_utils::builder`](crate::builder()).

use {
    std::{
        collections::HashSet,
        future::Future,
        iter,
        pin::Pin,
        sync::Arc,
        time::Duration,
    },
    serenity::{
        all::{
            CreateMessage,
            ClientBuilder,
            Http,
        },
        framework::standard::{
            Args,
            CommandGroup,
            CommandResult,
            HelpOptions,
            StandardFramework,
            help_commands,
            macros::help,
        },
        model::prelude::*,
        prelude::*,
    },
    tokio::time::sleep,
    crate::{
        RwFuture,
        handler::{
            self,
            Handler,
            HandlerMethods,
        },
    },
};

/// Select where to notify about errors, e.g. in [`task`](Builder::task)s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorNotifier {
    /// Write the error to standard error. This is the default.
    Stderr,
    /// Post in a Discord channel.
    Channel(ChannelId),
    /// DM a Discord user.
    User(UserId),
}

impl ErrorNotifier {
    pub(crate) async fn say(&self, ctx: &Context, msg: String) -> serenity::Result<()> {
        match self {
            ErrorNotifier::Stderr => eprintln!("{msg}"),
            ErrorNotifier::Channel(channel) => { channel.say(ctx, msg).await?; }
            ErrorNotifier::User(user) => { user.to_user(ctx).await?.dm(ctx, CreateMessage::new().content(msg)).await?; }
        }
        Ok(())
    }
}

impl TypeMapKey for ErrorNotifier {
    type Value = Self;
}

enum PlainMessage {}

impl TypeMapKey for PlainMessage {
    type Value = for<'a> fn(&'a Context, &'a Message) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

enum UnrecognizedReply {}

impl TypeMapKey for UnrecognizedReply {
    type Value = String;
}

/// A builder for setting up and running a bot.
///
/// This type is created using the [`builder`](crate::builder()) function, and used by returning it from a function annotated with [`serenity_utils::main`](crate::main).
pub struct Builder {
    client: ClientBuilder,
    /// Resolves to the [`Context`] once the bot is ready. This can be used to make the bot do things from other parts of the program.
    pub ctx_fut: RwFuture<Context>,
    framework: StandardFramework,
    handler: Handler,
    intents: GatewayIntents,
}

impl Builder {
    pub(crate) async fn new(app_id: impl Into<ApplicationId>, token: String) -> serenity::Result<Self> {
        let app_info = Http::new(&token).get_current_application_info().await?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut handler = Handler::default();
        handler.ctx_tx = Some(Arc::new(Mutex::new(Some(tx))));
        let framework = StandardFramework::new();
        framework.configure(|c| c
            .with_whitespace(true)
            .case_insensitivity(true)
            .no_dm_prefix(true)
            .on_mention(Some(UserId(app_info.id.0)))
            .owners(iter::once(app_info.owner.id).collect())
        );
        let builder = Self {
            client: Client::builder(&token, GatewayIntents::default()).application_id(app_id.into()),
            ctx_fut: RwFuture::new(async move { rx.await.expect("failed to store handler context") }),
            framework: framework.after(|ctx, msg, command_name, result| Box::pin(async move {
                if let Err(why) = result {
                    if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                        let _ = error_notifier.say(ctx, format!("Command '{}' from {} returned error `{:?}`", command_name, msg.author.tag(), why)).await;
                    }
                    let _ = msg.reply(ctx, &format!("an error occurred while handling your command: {:?}", why)).await;
                }
            })),
            intents: GatewayIntents::empty(),
            handler,
        };
        builder
            .error_notifier(ErrorNotifier::Stderr)
            .unrecognized_message("sorry, I don't understand that message")
            .ok()
    }

    /// Inserts a value into [`Context::data`].
    pub fn data<T: TypeMapKey>(mut self, value: T::Value) -> Self {
        self.client = self.client.type_map_insert::<T>(value);
        self
    }

    /// Changes how the bot will notify about errors.
    ///
    /// The default is no action.
    pub fn error_notifier(self, notifier: ErrorNotifier) -> Self {
        self.data::<ErrorNotifier>(notifier)
    }

    /// Adds command handling via [`serenity`]'s [`StandardFramework`] with a useful default configuration.
    pub fn message_commands(mut self, prefix: Option<&str>, commands: &'static CommandGroup) -> Self {
        #[help]
        async fn help(ctx: &Context, msg: &Message, args: Args, help_options: &'static HelpOptions, groups: &[&'static CommandGroup], owners: HashSet<UserId>) -> CommandResult {
            let _ = help_commands::with_embeds(ctx, msg, args, help_options, groups, owners).await;
            Ok(())
        }

        if let Some(prefix) = prefix {
            self.framework.configure(|c| c.prefix(prefix));
        }
        self.framework = self.framework
            .help(&HELP)
            .group(commands);
        self.intents |= GatewayIntents::DIRECT_MESSAGES | GatewayIntents::GUILD_MESSAGES;
        self
    }

    /// Sets the reply content for unrecognized messages in DMs.
    ///
    /// The default is “sorry, I don't understand that message”.
    pub fn unrecognized_message(self, text: impl ToString) -> Self {
        self.data::<UnrecognizedReply>(text.to_string())
    }

    /// If the given function returns `false` and the message is a DM, the “unrecognized command” reply is sent.
    pub fn plain_message(mut self, f: for<'a> fn(&'a Context, &'a Message) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>) -> Self {
        self = self.data::<PlainMessage>(f);
        self.framework = self.framework
            .normal_message(|ctx, msg| Box::pin(async move {
                if msg.author.bot { return; } // ignore bots to prevent message loops
                if !msg.is_private() {
                    let data = ctx.data.read().await;
                    let f = data.get::<PlainMessage>().expect("missing PlainMessage data");
                    let _ = f(ctx, msg).await;
                }
            }))
            .unrecognised_command(|ctx, msg, _| Box::pin(async move {
                if msg.author.bot { return; } // ignore bots to prevent message loops
                if msg.is_private() {
                    let data = ctx.data.read().await;
                    let f = data.get::<PlainMessage>().expect("missing PlainMessage data");
                    if !f(ctx, msg).await {
                        let unrecognized_reply = data.get::<UnrecognizedReply>().expect("missing UnrecognizedReply data");
                        msg.reply(ctx, unrecognized_reply).await.expect("failed to reply to unrecognized DM");
                    }
                }
            }));
        self
    }

    /// Adds intents.
    ///
    /// This normally doesn't need to be called explicitly since intents required for registered handler methods are set automatically.
    /// Only use this if you need additional intents for API calls.
    pub fn add_intents(mut self, new_intents: GatewayIntents) -> Self {
        self.intents |= new_intents;
        self
    }

    /// Adds an event handler.
    ///
    /// This can be called multiple times and/or combined with [`HandlerMethods`] trait methods; all methods are called in the order they were added.
    pub fn event_handler(mut self, handler: Handler) -> Self {
        self.handler.merge(handler);
        self
    }

    /// Spawns a task that will receive access to the [`Context`] once the bot is ready.
    ///
    /// This can be used to have the bot react to events coming from outside of Discord.
    pub fn task<
        Fut: Future<Output = ()> + Send + 'static,
        F: FnOnce(RwFuture<Context>, Box<dyn Fn(String, Box<dyn std::error::Error + Send + 'static>, Option<Duration>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>) -> Fut,
    >(self, task_fn: F) -> Self {
        let ctx_fut = self.ctx_fut.clone();
        tokio::spawn(task_fn(ctx_fut.clone(), Box::new(move |thread_kind, e, auto_retry| {
            let ctx_fut = ctx_fut.clone();
            Box::pin(async move {
                let ctx = ctx_fut.read().await;
                if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                    error_notifier.say(&*ctx, format!(
                        "{} thread crashed: {} (`{:?}`), {}",
                        thread_kind,
                        e,
                        e,
                        if let Some(auto_retry) = auto_retry { format!("auto-retrying in `{:?}`", auto_retry) } else { format!("**not** auto-retrying") },
                    )).await.expect("failed to send thread crash notification");
                };
            })
        })));
        self
    }

    /// Convenience method wrapping `self` in [`Ok`] which can be used at the end of a method call chain.
    pub fn ok<E>(self) -> Result<Self, E> { Ok(self) }

    #[doc(hidden)] pub async fn run(mut self) -> serenity::Result<()> { // used in `serenity_utils::main`
        self.intents |= self.handler.intents;
        self.client = self.client.event_handler(self.handler);
        let mut client = self.client
            .framework(self.framework)
            .intents(self.intents)
            .await?; // build the client
        {
            let mut data = client.data.write().await;
            data.insert::<crate::ShardManagerContainer>(Arc::clone(&client.shard_manager));
        }
        client.start_autosharded().await?;
        sleep(Duration::from_secs(1)).await; // wait to make sure websockets can be closed cleanly
        Ok(())
    }
}

impl HandlerMethods for Builder {
    fn on_ready(mut self, f: for<'r> fn(&'r Context, &'r Ready) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_ready(f);
        self
    }

    fn on_guild_ban_addition(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_ban_addition(f);
        self
    }

    fn on_guild_ban_removal(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_ban_removal(f);
        self
    }

    fn on_guild_create(mut self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, Option<bool>) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_create(require_members, f);
        self
    }

    fn on_guild_member_addition(mut self, f: for<'r> fn(&'r Context, &'r Member) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_member_addition(f);
        self
    }

    fn on_guild_member_removal(mut self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_member_removal(f);
        self
    }

    fn on_guild_member_update(mut self, f: for<'r> fn(&'r Context, Option<&'r Member>, Option<&'r Member>, &'r GuildMemberUpdateEvent) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_member_update(f);
        self
    }

    fn on_guild_members_chunk(mut self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_members_chunk(f);
        self
    }

    fn on_interaction_create(mut self, f: for<'r> fn(&'r Context, &'r Interaction) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_interaction_create(f);
        self
    }

    fn on_guild_role_create(mut self, f: for<'r> fn(&'r Context, &'r Role) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_guild_role_create(f);
        self
    }

    fn on_message(mut self, require_content: bool, f: for<'r> fn(&'r Context, &'r Message) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_message(require_content, f);
        self
    }

    fn on_voice_state_update(mut self, f: for<'r> fn(&'r Context, Option<&'r VoiceState>, &'r VoiceState) -> handler::Output<'r>) -> Self {
        self.handler = self.handler.on_voice_state_update(f);
        self
    }
}
