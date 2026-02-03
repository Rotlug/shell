use crate::user::User;

#[derive(Debug)]
pub enum Event {
    UserJoined(User),
    UserLeft(String),
}
