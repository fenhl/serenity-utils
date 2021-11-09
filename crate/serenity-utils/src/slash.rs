//! Utilities for working with [slash commands, also known as application commands](https://discord.com/developers/docs/interactions/application-commands).

use {
    std::ops::BitOr,
    serenity::model::prelude::*,
};
pub use serenity::model::interactions::application_command::{
    ApplicationCommandInteractionDataOption,
    ApplicationCommandInteractionDataOptionValue,
    ApplicationCommandOptionType,
};

/// Specifies who has permission to call a slash command.
///
/// Passed as a parameter to [`Builder::slash_command`](crate::Builder::).
///
/// By [`default`](Self::default), no one is allowed to use the command.
#[derive(Default)]
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
