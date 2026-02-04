use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::{
    sync::mpsc::{Receiver, Sender, channel},
    task::JoinHandle,
    time,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{event::Event, user::User};

pub struct Listener {
    handles: Vec<JoinHandle<()>>,
}

impl Listener {
    #[must_use]
    pub fn new(url: String) -> (Self, Receiver<Event>) {
        let (event_tx, event_rx) = channel(16);
        let mut handles = vec![];

        //receiver just prints whatever it gets
        let recv_task = tokio::spawn(async move {
            let mut old_user_list: Vec<User> = vec![];

            let mut backoff = 1; // Exponential backoff

            loop {
                let ws_result = connect_async(&url).await;

                match ws_result {
                    Ok((ws_stream, _)) => {
                        // Connection Successful, reset backoff
                        backoff = 1;

                        let (mut sender, mut receiver) = ws_stream.split();

                        let _ = sender.send(Message::Text("Hi server :)".into())).await;

                        while let Some(Ok(msg)) = receiver.next().await {
                            if let Message::Text(text) = msg {
                                // Parse json into Vec<User> and send diff to rx
                                if let Ok(new_user_list) = serde_json::from_str::<Vec<User>>(&text)
                                {
                                    send_user_events(&old_user_list, &new_user_list, &event_tx)
                                        .await;
                                    old_user_list = new_user_list;
                                }
                            }
                        }

                        // Connection closed.
                        eprintln!("Connection lost, retrying in {backoff} seconds");
                    }
                    Err(e) => {
                        eprintln!("Connection failed: {e}, retrying in {backoff} seconds");
                    }
                }

                // Retry connection in 60s max
                time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        });

        handles.push(recv_task);

        (Self { handles }, event_rx)
    }
}

async fn send_user_events(old_list: &[User], new_list: &[User], event_tx: &Sender<Event>) {
    for user in new_list {
        if !old_list.contains(user) {
            let _ = event_tx.send(Event::UserJoined(user.clone())).await;
        }
    }

    for user in old_list {
        if !new_list.contains(user) {
            let _ = event_tx.send(Event::UserLeft(user.name.clone())).await;
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
