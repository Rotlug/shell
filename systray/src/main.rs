use std::collections::HashMap;

use systray::{event::Event, tray::Tray};

#[tokio::main]
async fn main() -> systray::Result<()> {
    let (tray, mut rx) = Tray::new().await?;

    let mut items = HashMap::new();

    while let Some(event) = rx.recv().await {
        match event {
            Event::ItemAdded(addr, item) => {
                println!("Welcome {:?}", item.title().await);
                items.insert(addr, item);
            }
            Event::ItemRemoved(addr) => {
                println!("{addr} has been removed");
            }
            Event::MenuChanged(addr, items) => {
                println!("New items for {addr}:");
                for item in items {
                    println!("{:?}", item.contents);
                }
            }
            Event::IconChanged(addr) => {
                println!("New icon for {addr}");
            }
        }
    }

    Ok(())
}
