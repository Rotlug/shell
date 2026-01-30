use std::time::Duration;

use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

/* Urgency */
#[derive(Debug, Default, Clone)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Urgent,
}

impl From<u64> for Urgency {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::Low,
            1 => Self::Normal,
            2 => Self::Urgent,
            _ => Self::default(),
        }
    }
}

/* Closing Reason */
#[derive(Default, Debug, Clone, Serialize, Deserialize, Type)]
#[serde(into = "u32", from = "u32")]
pub enum ClosingReason {
    Expired,
    Dismissed,
    Inner,
    #[default]
    Undefined,
}

impl From<ClosingReason> for u32 {
    fn from(value: ClosingReason) -> Self {
        match value {
            ClosingReason::Expired => 1,
            ClosingReason::Dismissed => 2,
            ClosingReason::Inner => 3,
            ClosingReason::Undefined => 4,
        }
    }
}

impl From<u32> for ClosingReason {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Expired,
            2 => Self::Dismissed,
            3 => Self::Inner,
            _ => Self::default(),
        }
    }
}

/* Notification */
#[derive(Debug, Clone)]
pub struct Notification {
    /// The notifications id
    pub id: u32,
    /// Name of the application that sent the notification
    pub app_name: String,
    /// The notification icon
    pub app_icon: String,
    /// Title text
    pub summary: String,
    /// Body text
    pub body: String,
    /// The duration that the notification is on screen
    pub expire_timeout: Option<Duration>,
    /// Urgency level
    pub urgency: Urgency,
    /// Timestamp of when the notification has been created
    pub timestamp: u64,
    /// The actions the user can take on the notifications (buttons)
    pub actions: Vec<String>,
}
