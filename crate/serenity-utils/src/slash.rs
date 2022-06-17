//! Utilities for working with [slash commands, also known as application commands](https://discord.com/developers/docs/interactions/application-commands).

use {
    std::{
        fmt,
        future::Future,
        ops::BitOr,
        pin::Pin,
    },
    serenity::{
        builder::CreateApplicationCommand,
        model::prelude::*,
        prelude::*,
    },
};
pub use serenity::model::interactions::application_command::*;

/// A slash command.
///
/// Usually constructed using [`serenity_utils::slash_command`](serenity_utils_derive::slash_command).
#[derive(Clone)]
pub struct Command {
    /// The guild for which this command will be registered.
    ///
    /// Global slash commands are currently unsupported.
    pub guild_id: GuildId,
    /// The command name. Must be unique for this application and guild.
    pub name: &'static str,
    /// The permissions that will be set up for the command.
    pub perms: fn() -> CommandPermissions,
    /// The command will be created with these options. The `name` must be set here.
    ///
    /// If it already exists, these options will override the existing ones.
    pub setup: fn(&mut CreateApplicationCommand) -> &mut CreateApplicationCommand,
    /// The function to be called when the command is used.
    pub handle: for<'r> fn(&'r Context, ApplicationCommandInteraction) -> crate::handler::Output<'r>,
}

/// Specifies who has permission to call a slash command.
///
/// Part of a [`Command`].
///
/// By [`default`](Self::default), no one is allowed to use the command.
#[derive(Default, Clone)]
pub struct CommandPermissions {
    pub(crate) roles: Vec<RoleId>,
    pub(crate) users: Vec<UserId>,
}

impl From<RoleId> for CommandPermissions {
    fn from(role_id: RoleId) -> Self {
        Self {
            roles: vec![role_id],
            ..Self::default()
        }
    }
}

impl From<UserId> for CommandPermissions {
    fn from(user_id: UserId) -> Self {
        Self {
            users: vec![user_id],
            ..Self::default()
        }
    }
}

impl<R: Into<Self>> BitOr<R> for CommandPermissions {
    type Output = Self;

    fn bitor(mut self, rhs: R) -> Self {
        let Self { roles, users } = rhs.into();
        self.roles.extend(roles);
        self.users.extend(users);
        self
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub enum ParseError { // used in proc macro
    IntegerRange,
    OptionName,
    OptionType,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntegerRange => write!(f, "integer option out of range"),
            Self::OptionName => write!(f, "unexpected option name"),
            Self::OptionType => write!(f, "unexpected option type"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A type that can be returned from a [`serenity_utils::slash_command`](serenity_utils_derive::slash_command) function (or the future it returns).
pub trait Responder<'a> {
    /// Sends a response for the interaction or returns an error.
    fn respond(self, ctx: &'a Context, interaction: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;
}

/// Return this from a slash command to skip creating the interaction response.
///
/// Note that users will see commands that haven't been responded to as failed.
pub struct NoResponse;

impl<'a> Responder<'a> for NoResponse {
    fn respond(self, _: &'a Context, _: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

impl<'a> Responder<'a> for () {
    fn respond(self, ctx: &'a Context, interaction: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            interaction.create_interaction_response(ctx, |builder| builder.interaction_response_data(|data| data.content("success").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL))).await?;
            Ok(())
        })
    }
}

impl<'a> Responder<'a> for String {
    fn respond(self, ctx: &'a Context, interaction: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            interaction.create_interaction_response(ctx, |builder| builder.interaction_response_data(|data| data.content(self).flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL))).await?;
            Ok(())
        })
    }
}

impl<'a, 'b: 'a> Responder<'a> for &'b str {
    fn respond(self, ctx: &'a Context, interaction: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            interaction.create_interaction_response(ctx, |builder| builder.interaction_response_data(|data| data.content(self).flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL))).await?;
            Ok(())
        })
    }
}

impl<'a, T: Responder<'a> + Send + 'a, E: std::error::Error + Send + Sync + 'static> Responder<'a> for Result<T, E> {
    fn respond(self, ctx: &'a Context, interaction: &'a ApplicationCommandInteraction) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>> {
        Box::pin(async move {
            match self {
                Ok(x) => x.respond(ctx, interaction).await,
                Err(e) => Err(e.into()),
            }
        })
    }
}
