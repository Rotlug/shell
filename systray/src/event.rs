use std::sync::Arc;

use crate::{item::Item, menu::MenuItem};

#[derive(Debug)]
pub enum Event {
    ItemAdded(String, Arc<Item>),
    ItemRemoved(String),
    MenuChanged(String, Vec<MenuItem>),
    IconChanged(String),
}
