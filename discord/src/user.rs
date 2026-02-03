use serde::{Deserialize, Serialize};

/// Represents a discord user, as is sent through the discord-applet server.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct User {
    /// The url of the users profile picture
    pub avatar: String,
    /// The user's display name.
    pub name: String,
}
