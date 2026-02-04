use std::{sync::Arc, thread, time::Duration};

use notif_daemon::{event::Event, manager::Manager, notification::ClosingReason};

#[tokio::main]
async fn main() -> notif_daemon::Result<()> {
    let (mgr, mut rx) = Manager::new().await?;
    let mgr = Arc::new(mgr);

    while let Some(event) = rx.recv().await {
        dbg!(&event);
        match event {
            Event::Incoming(notif) => {
                let mgr_clone = mgr.clone();
                tokio::spawn(async move {
                    thread::sleep(Duration::from_secs(3));
                    mgr_clone
                        .close_notification(notif.id, ClosingReason::Inner)
                        .await;
                });
            }
            _ => {}
        }
    }

    Ok(())
}
