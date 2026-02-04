use discord::Listener;

#[tokio::main]
async fn main() -> discord::Result<()> {
    let (listener, mut rx) = Listener::new("ws://localhost:49152".to_string());

    while let Some(event) = rx.recv().await {
        dbg!(event);
    }

    Ok(())
}
