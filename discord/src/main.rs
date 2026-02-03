use discord::listener::Listener;

#[tokio::main]
async fn main() -> discord::Result<()> {
    let (listener, rx) = Listener::new("ws://localhost:49152".to_string());

    while let Ok(event) = rx.recv() {
        dbg!(event);
    }

    Ok(())
}
