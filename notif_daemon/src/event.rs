use crate::notification::Notification;

#[derive(Debug)]
pub enum Event {
    Incoming(Notification),
    Closed(u32),
}

#[derive(Debug)]
pub enum Command {
    Close(u32),
    Action(u32, String),
}
