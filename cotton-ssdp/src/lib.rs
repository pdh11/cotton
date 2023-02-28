//! Implementing SSDP, the Simple Service Discovery Protocol
//!
//! The cotton-ssdp crate encapsulates a client and server for the
//! Simple Service Discovery Protocol (SSDP), a mechanism for
//! discovering available _resources_ (services) on local networks. A
//! _resource_ might be a streaming-media server, or a router, or a
//! network printer, or anything else that someone might want to
//! search for or enumerate on a network.
//!
//! What is advertised, or discovered, is, for each
//! resource, a unique identifier (Unique Service Name, USN), an
//! identifier for the _type_ of resource (Notification Type, NT), and
//! the _location_ of the resource in the form of a URL.
//!
//! SSDP is mainly used by UPnP (Universal Plug-'n'-Play) systems,
//! such as for media libraries and local streaming of music and video
//! -- but the mechanism is quite generic, and could as easily be used
//! for any type of device or resource that must be discoverable over
//! a network, including in *ad hoc* settings which don't necessarily
//! have expert network administrators close at hand.
//!
//! There is no Internet RFC as such for SSDP -- merely some expired
//! drafts.  The protocol is, instead, documented in the [UPnP Device
//! Architecture](https://openconnectivity.org/developer/specifications/upnp-resources/upnp/archive-of-previously-published-upnp-device-architectures/)
//! documents.
//!
//! This crate provides two different high-level interfaces for
//! working with SSDP, [`Service`] and [`AsyncService`]. Either one can
//! be used both to discover other devices ([`Service::subscribe`])
//! and to advertise resources itself ([`Service::advertise`]).
//!
//! Client code using the MIO crate should use plain [`Service`];
//! client code using the Tokio crate might wish to use
//! [`AsyncService`] instead, which integrates with that
//! system. Client code with a _custom_ polling loop -- neither MIO
//! nor Tokio -- should instead probably aim to build an equivalent to
//! [`Service`] using the lower-level facilities in
//! [`engine::Engine`].
//!
//! Example code is available both for asynchronous Tokio use:
//! [ssdp-search](https://github.com/pdh11/cotton/blob/main/cotton-ssdp/examples/ssdp-search.rs)
//! (on Github) and reactor-style MIO use:
//! [ssdp-search-mio](https://github.com/pdh11/cotton/blob/main/cotton-ssdp/examples/ssdp-search-mio.rs)
//! (on Github).

//#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

/// SSDP Notification Subtype
///
/// SSDP [`Notification`] messages are sent on both arrival and
/// departure of network resources. Arrivals are distinguished from
/// departures by the notification subtype: either "Alive" or
/// "Bye-bye".
///
/// Only notifications of alive (arriving) resources have a Location
/// field, so this is expressed in the enum.
#[derive(Debug, Clone)]
pub enum NotificationSubtype {
    /// The resource in question is now active (at this location/URL)
    AliveLocation(String),

    /// The resource in question is (becoming) inactive
    ByeBye,
}

/// Incoming SSDP notification, obtained from [`Service::subscribe`]
///
/// Sent in response to searches, and when new resources are made
/// available, and periodically otherwise just in case.
///
/// Neither [`Service`] nor [`AsyncService`] de-duplicates these
/// notifications -- in other words, a caller of
/// [`Service::subscribe`] is likely to receive multiple copies of
/// each. The `unique_service_name` field can be used to distinguish
/// genuinely new resources (e.g., as the key in a `HashMap`).
#[derive(Debug, Clone)]
pub struct Notification {
    pub notification_type: String,
    pub unique_service_name: String,
    pub notification_subtype: NotificationSubtype,
}

/// Outgoing SSDP announcement, passed to [`Service::advertise`]
pub struct Advertisement {
    pub notification_type: String,
    pub location: url::Url,
}

mod async_service;

/// Low-level SSDP API used inside [`Service`] and [`AsyncService`]
pub mod engine;
mod message;
mod service;

/// Traits used to abstract over various UDP socket implementations
pub mod udp;

pub use async_service::AsyncService;
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
