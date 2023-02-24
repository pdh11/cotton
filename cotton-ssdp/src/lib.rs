//! Implementing SSDP, the Simple Service Discovery Protocol
//!
//! The cotton-ssdp crate encapsulates a client and server for the
//! Simple Service Discovery Protocol (SSDP), a mechanism for
//! discovering available resources on local networks. What is
//! advertised, or discovered, is, for each resource, a unique
//! identifier (Unique Service Name, USN), an identifier for the type
//! of resource (Notification Type, NT), and the location of the
//! resource in the form of a URL.
//!
//! SSDP is mainly used by UPnP (Universal Plug-'n'-Play) systems,
//! such as for media libraries and local streaming of music and video
//! -- but the mechanism is quite generic, and could as easily be used
//! for any type of device or resource that must be discoverable over
//! a network, including in ad hoc settings which don't necessarily
//! have expert network administrators close at hand.
//!
//! The crate provides two different interfaces for working with SSDP,
//! `Service` and `AsyncService`. Either one can be used both to
//! discover other devices (`Service::search`) and to advertise
//! resources itself (`Service::advertise`).
//!
//! Client code using the MIO crate, or a custom polling loop, should
//! use plain `Service`; client code using the Tokio crate might wish
//! to use `AsyncService` instead, which integrates with that system.

//#![warn(missing_docs)] // @todo
#![warn(rustdoc::missing_crate_level_docs)]

#[derive(Debug, Clone)]
pub enum NotificationSubtype {
    AliveLocation(String),
    ByeBye,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub notification_type: String,
    pub unique_service_name: String,
    pub notification_subtype: NotificationSubtype,
}

pub struct Advertisement {
    pub notification_type: String,
    pub location: url::Url,
}

pub mod engine;
mod message;
mod service;
pub mod udp;

pub use service::Service;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_debug() {
        println!(
            "{:?}",
            Notification {
                notification_type: String::new(),
                unique_service_name: String::new(),
                notification_subtype: NotificationSubtype::AliveLocation(
                    String::new()
                ),
            }
        );
    }

    #[test]
    #[allow(clippy::redundant_clone)]
    fn can_clone() {
        let _ = Notification {
            notification_type: String::new(),
            unique_service_name: String::new(),
            notification_subtype: NotificationSubtype::AliveLocation(
                String::new(),
            ),
        }
        .clone();
    }
}
