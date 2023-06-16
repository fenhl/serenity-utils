//! Extensions for serenity's [`MessageBuilder`] type.

use {
    chrono::prelude::*,
    serenity::{
        all::MessageBuilder,
        model::prelude::*,
    },
};

/// The ways timestamps in Discord messages can be formatted. [Discord docs](https://discord.com/developers/docs/reference#message-formatting-timestamp-styles)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimestampStyle {
    /// e.g. `20/04/2021`
    ShortDate,
    /// e.g. `20 April 2021`
    LongDate,
    /// e.g. `16:20`
    ShortTime,
    /// e.g. `16:20:30`
    LongTime,
    /// e.g. `20 April 2021 16:20`
    #[default]
    ShortDateTime,
    /// e.g. `Tuesday, 20 April 2021 16:20`
    LongDateTime,
    /// e.g. `2 months ago`
    Relative,
}

impl TimestampStyle {
    fn to_char(&self) -> char {
        match self {
            Self::ShortDate => 'd',
            Self::LongDate => 'D',
            Self::ShortTime => 't',
            Self::LongTime => 'T',
            Self::ShortDateTime => 'f',
            Self::LongDateTime => 'F',
            Self::Relative => 'R',
        }
    }
}

/// Extends [`MessageBuilder`] with additional features supported by Discord but not implemented in serenity.
pub trait MessageBuilderExt {
    /// Appends a clickable link to a slash command to the message. `name` must be the exact command name, otherwise it may not be clickable.
    fn mention_command(&mut self, command_id: CommandId, name: &str) -> &mut Self;
    /// Formats the given date and time according to the viewer's locale and the given style.
    fn push_timestamp<Z: TimeZone>(&mut self, timestamp: DateTime<Z>, format: TimestampStyle) -> &mut Self;
}

impl MessageBuilderExt for MessageBuilder {
    fn mention_command(&mut self, command_id: CommandId, name: &str) -> &mut Self {
        self.push("</").push(name).push(':').push(command_id.to_string()).push('>')
    }

    fn push_timestamp<Z: TimeZone>(&mut self, timestamp: DateTime<Z>, format: TimestampStyle) -> &mut Self {
        self.push("<t:")
            .push(timestamp.timestamp().to_string())
            .push(":")
            .push(format.to_char().to_string())
            .push(">")
    }
}
