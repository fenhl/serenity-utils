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
        client::{
            ClientBuilder,
            bridge::gateway::GatewayIntents,
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
        http::Http,
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
            ErrorNotifier::Stderr => eprintln!("{}", msg),
            ErrorNotifier::Channel(channel) => { channel.say(ctx, msg).await?; }
            ErrorNotifier::User(user) => { user.to_user(ctx).await?.dm(ctx, |m| m.content(msg)).await?; }
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
    client: ClientBuilder<'static>,
    ctx_fut: Option<RwFuture<Context>>,
    framework: StandardFramework,
    handler: Option<Handler>,
    intents: GatewayIntents,
}

impl Builder {
    pub(crate) async fn new(token: String) -> serenity::Result<Self> {
        let app_info = Http::new_with_token(&token).get_current_application_info().await?;
        let builder = Self {
            client: Client::builder(&token),
            ctx_fut: None,
            framework: StandardFramework::new()
                .configure(|c| c
                    .with_whitespace(true)
                    .case_insensitivity(true)
                    .no_dm_prefix(true)
                    .on_mention(Some(app_info.id))
                    .owners(iter::once(app_info.owner.id).collect())
                )
                .after(|ctx, msg, command_name, result| Box::pin(async move {
                    if let Err(why) = result {
                        if let Some(error_notifier) = ctx.data.read().await.get::<ErrorNotifier>() {
                            let _ = error_notifier.say(ctx, format!("Command '{}' from {} returned error `{:?}`", command_name, msg.author.tag(), why)).await;
                        }
                        let _ = msg.reply(ctx, &format!("an error occurred while handling your command: {:?}", why)).await;
                    }
                })),
            handler: None,
            intents: GatewayIntents::empty(),
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

    /// Adds command handling with a useful default configuration.
    pub fn commands(mut self, prefix: Option<&str>, commands: &'static CommandGroup) -> Self {
        #[help]
        async fn help(ctx: &Context, msg: &Message, args: Args, help_options: &'static HelpOptions, groups: &[&'static CommandGroup], owners: HashSet<UserId>) -> CommandResult {
            let _ = help_commands::with_embeds(ctx, msg, args, help_options, groups, owners).await;
            Ok(())
        }

        if let Some(prefix) = prefix {
            self.framework = self.framework.configure(|c| c.prefix(prefix));
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

    /// Adds an event handler.
    ///
    /// This can be called multiple times and/or combined with [`HandlerMethods`] trait methods; all methods are called in the order they were added.
    pub fn event_handler(self, handler: Handler) -> Self {
        self.edit_handler(|mut my_handler| {
            my_handler.merge(handler);
            my_handler
        })
    }

    /// Directly sets the `serenity` event handler for the bot.
    ///
    /// Since the intents used by the event handler cannot be determined, they must be specified explicitly.
    #[deprecated]
    pub fn raw_event_handler(mut self, handler: impl EventHandler + 'static, intents: GatewayIntents) -> Self {
        self.client = self.client.event_handler(handler);
        self.intents |= intents;
        self
    }

    /// Directly sets the `serenity` event handler for the bot.
    ///
    /// Since the intents used by the event handler cannot be determined, they must be specified explicitly.
    #[deprecated]
    pub fn raw_event_handler_with_ctx<H: EventHandler + 'static, F: FnOnce() -> (H, RwFuture<Context>)>(mut self, make_handler: F, intents: GatewayIntents) -> Self {
        let (handler, ctx_fut) = make_handler();
        self.client = self.client.event_handler(handler);
        self.ctx_fut = Some(ctx_fut);
        self.intents |= intents;
        self
    }

    /// # Panics
    ///
    /// If `raw_event_handler_with_ctx` has not been called on this builder.
    pub fn task<
        Fut: Future<Output = ()> + Send + 'static,
        F: FnOnce(RwFuture<Context>, Box<dyn Fn(String, Box<dyn std::error::Error + Send + 'static>, Option<Duration>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>) -> Fut,
    >(self, task_fn: F) -> Self {
        let ctx_fut = self.ctx_fut.as_ref().expect("serenity_utils::Builder::task requires serenity_utils::Builder::raw_event_handler_with_ctx").clone();
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

    #[doc(hidden)] pub fn has_ctx_fut(&self) -> bool {
        self.ctx_fut.is_some()
    }

    #[doc(hidden)] pub async fn run(mut self) -> serenity::Result<()> { // used in `serenity_utils::main`
        if let Some(handler) = self.handler {
            self.intents |= handler.intents;
            self.client = self.client.event_handler(handler);
        }
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

    fn edit_handler(mut self, f: impl FnOnce(Handler) -> Handler) -> Self {
        if let Some(handler) = self.handler {
            self.handler = Some(f(handler));
        } else {
            let (handler, ctx_fut) = Handler::new_with_ctx();
            self.handler = Some(f(handler));
            self.ctx_fut = Some(ctx_fut);
        }
        self
    }
}

impl HandlerMethods for Builder {
    fn on_ready(self, f: for<'r> fn(&'r Context, &'r Ready) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_ready(f))
    }

    fn on_guild_ban_addition(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_ban_addition(f))
    }

    fn on_guild_ban_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_ban_removal(f))
    }

    fn on_guild_create(self, require_members: bool, f: for<'r> fn(&'r Context, &'r Guild, bool) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_create(require_members, f))
    }

    fn on_guild_member_addition(self, f: for<'r> fn(&'r Context, GuildId, &'r Member) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_member_addition(f))
    }

    fn on_guild_member_removal(self, f: for<'r> fn(&'r Context, GuildId, &'r User, Option<&'r Member>) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_member_removal(f))
    }

    fn on_guild_member_update(self, f: for<'r> fn(&'r Context, Option<&'r Member>, &'r Member) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_member_update(f))
    }

    fn on_guild_members_chunk(self, f: for<'r> fn(&'r Context, &'r GuildMembersChunkEvent) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_guild_members_chunk(f))
    }

    fn on_message(self, f: for<'r> fn(&'r Context, &'r Message) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_message(f))
    }

    fn on_voice_state_update(self, f: for<'r> fn(&'r Context, Option<GuildId>, Option<&'r VoiceState>, &'r VoiceState) -> handler::Output<'r>) -> Self {
        self.edit_handler(|handler| handler.on_voice_state_update(f))
    }
}
