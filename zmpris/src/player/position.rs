use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
    time::{self, Instant},
};

use crate::{Update, generated::player::PlayerProxy};

#[derive(Debug)]
pub struct Position {
    handle: JoinHandle<((), ())>,
    position: Arc<Mutex<Duration>>,
}

impl Position {
    pub async fn new(
        proxy: PlayerProxy<'static>,
        mut rx: broadcast::Receiver<Update>,
        tx: broadcast::Sender<Update>,
    ) -> crate::Result<Self> {
        let pos = proxy.position().await?;

        #[allow(clippy::cast_sign_loss)]
        let last_pos = Arc::new(Mutex::new(Duration::from_micros(pos as u64)));
        let last_updated = Arc::new(Mutex::new(Instant::now()));
        let rate = Arc::new(Mutex::new(0.0));

        /* Task that listens to position updates */
        let pos_handle = last_pos.clone();
        let updated_handle = last_updated.clone();
        let rate_handle = rate.clone();
        let listener = async move {
            while let Ok(upd) = rx.recv().await {
                match upd {
                    Update::Position(pos) => {
                        *pos_handle.lock().await = pos;
                        *updated_handle.lock().await = Instant::now();
                    }
                    Update::PlaybackRate(rate) => {
                        *rate_handle.lock().await = rate;
                    }
                    _ => (),
                }
            }
        };

        /* Task that sends position updates once a second */
        let pos_handle = last_pos.clone();
        let updated_handle = last_updated;
        let rate_handle = rate;
        let updater = async move {
            loop {
                // Time elapsed since last update
                let elapsed = Instant::now().duration_since(*updated_handle.lock().await);

                *updated_handle.lock().await = Instant::now();

                *pos_handle.lock().await += elapsed.mul_f64(*rate_handle.lock().await);

                let _ = tx.send(Update::Position(*pos_handle.lock().await));
                time::sleep(Duration::from_secs(1)).await;
            }
        };

        let handle = tokio::spawn(async move { tokio::join!(listener, updater) });

        Ok(Self {
            handle,
            position: last_pos,
        })
    }

    pub async fn get_position(&self) -> Duration {
        *self.position.lock().await
    }
}

impl Drop for Position {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
