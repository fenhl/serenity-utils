//! Contains [`UserListExporter`], an [`EventHandler`] that maintains a record of each guild's list of users on disk.

use {
    std::{
        fs::{
            self,
            File, //TODO async
        },
        io::{
            self,
            prelude::*,
        },
        path::PathBuf,
    },
    async_trait::async_trait,
    serde_json::json,
    serenity::{
        model::prelude::*,
        prelude::*,
    },
};

/// An `EventHandler` which maintains a list of known Discord users present in guilds shared with the bot in a given directory.
pub struct UserListExporter {
    path: PathBuf
}

impl UserListExporter {
    /// Returns a new `UserListExporter` which writes to the given path.
    pub fn new(path: impl Into<PathBuf>) -> UserListExporter {
        UserListExporter {
            path: path.into()
        }
    }

    /// Add a Discord account to the given guild's user list.
    async fn add(&self, guild_id: GuildId, member: Member) -> io::Result<()> {
        let guild_dir = self.path.join(guild_id.to_string());
        if !guild_dir.exists() {
            fs::create_dir(&guild_dir)?;
        }
        let mut f = File::create(guild_dir.join(format!("{}.json", member.user.id)))?;
        write!(f, "{:#}", json!({
            "discriminator": member.user.discriminator,
            "snowflake": member.user.id,
            "username": member.user.name
        }))?;
        Ok(())
    }

    /// Remove a Discord account from the given guild's user list.
    async fn remove<U: Into<UserId>>(&self, guild_id: GuildId, user: U) -> io::Result<()> {
        match fs::remove_file(self.path.join(guild_id.to_string()).join(format!("{}.json", user.into()))) {
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r
        }
    }

    /// (Re)initialize the given guild's user list.
    async fn set_guild<I: IntoIterator<Item=Member>>(&self, guild_id: GuildId, members: I) -> io::Result<()> {
        let guild_dir = self.path.join(guild_id.to_string());
        if guild_dir.exists() {
            for entry in fs::read_dir(guild_dir)? {
                fs::remove_file(entry?.path())?;
            }
        }
        for member in members.into_iter() {
            self.add(guild_id, member).await?;
        }
        Ok(())
    }

    /// Update the data for a guild member. Equivalent to `remove` followed by `add`.
    async fn update(&self, guild_id: GuildId, member: Member) -> io::Result<()> {
        self.remove(guild_id, &member).await?;
        self.add(guild_id, member).await?;
        Ok(())
    }
}

#[async_trait]
impl EventHandler for UserListExporter {
    async fn guild_ban_addition(&self, _: Context, guild_id: GuildId, user: User) {
        self.remove(guild_id, user).await.expect("failed to remove banned user from user list");
    }

    async fn guild_ban_removal(&self, ctx: Context, guild_id: GuildId, user: User) {
        self.add(guild_id, guild_id.member(ctx, user).await.expect("failed to get unbanned guild member")).await.expect("failed to add unbanned user to user list");
    }

    async fn guild_create(&self, _: Context, guild: Guild, _: bool) {
        self.set_guild(guild.id, guild.members.values().cloned()).await.expect("failed to initialize user list");
    }

    async fn guild_member_addition(&self, _: Context, guild_id: GuildId, member: Member) {
        self.add(guild_id, member).await.expect("failed to add new guild member to user list");
    }

    async fn guild_member_removal(&self, _: Context, guild_id: GuildId, user: User, _: Option<Member>) {
        self.remove(guild_id, user).await.expect("failed to remove removed guild member from user list");
    }

    async fn guild_member_update(&self, _: Context, _: Option<Member>, member: Member) {
        self.update(member.guild_id, member).await.expect("failed to update guild member info in user list");
    }

    async fn guild_members_chunk(&self, _: Context, chunk: GuildMembersChunkEvent) {
        for member in chunk.members.values() {
            self.add(chunk.guild_id, member.clone()).await.expect("failed to add chunk of guild members to user list");
        }
    }
}
