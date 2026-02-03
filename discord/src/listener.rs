use std::sync::mpsc::{Receiver, Sender, channel};

use futures::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{event::Event, user::User};

pub struct Listener {
    handles: Vec<JoinHandle<()>>,
}

impl Listener {
    pub async fn new(url: &str) -> crate::Result<(Self, Receiver<Event>)> {
        let (event_tx, event_rx) = channel();
        let mut handles = vec![];

        let (ws_stream, _) = connect_async(url).await?;

        let (mut sender, mut receiver) = ws_stream.split();

        // Ping server to get updates.
        sender.send(Message::Text("Hello server :)".into())).await?;

        //receiver just prints whatever it gets
        let recv_task = tokio::spawn(async move {
            let mut old_user_list: Vec<User> = vec![];

            while let Some(Ok(msg)) = receiver.next().await {
                if let Message::Text(text) = msg {
                    let Ok(new_user_list) = serde_json::from_str::<Vec<User>>(text.as_str()) else {
                        continue;
                    };

                    send_user_events(&old_user_list, &new_user_list, &event_tx);

                    old_user_list = new_user_list;
                }
            }
        });

        handles.push(recv_task);

        Ok((Self { handles }, event_rx))
    }
}

fn send_user_events(old_list: &[User], new_list: &[User], event_tx: &Sender<Event>) {
    for user in new_list {
        if !old_list.contains(user) {
            let _ = event_tx.send(Event::UserJoined(user.clone()));
        }
    }

    for user in old_list {
        if !new_list.contains(user) {
            let _ = event_tx.send(Event::UserLeft(user.name.clone()));
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}
