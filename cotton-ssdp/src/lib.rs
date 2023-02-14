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
//! The crate provides an SSDP `Service`, which can be used both to
//! discover other devices (`Service::search`) and to advertise
//! resources itself (`Service::advertise`).
//!
//! The `Service` facility is easy to understand, but is synchronous
//! and blocking, so is not convenient for some use cases. Client code
//! using the Tokio crate might wish to use `AsyncService` instead,
//! which integrates with that system; client code using the MIO crate
//! might wish to use `ActorService`.

//#![warn(missing_docs)] // @todo
#![warn(rustdoc::missing_crate_level_docs)]

#[derive(Debug)]
pub struct Alive {
    pub notification_type: String,
    pub unique_service_name: String,
    pub location: String,
}

#[derive(Debug)]
pub struct ByeBye {
    pub notification_type: String,
    pub unique_service_name: String,
}

/** An incoming SSDP search
 *
 * Might match a USN or an NT or might be "ssdp:all".
 */
#[derive(Debug)]
pub struct Search {
    pub search_target: String,
    pub maximum_wait_sec: u8,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub search_target: String,
    pub unique_service_name: String,
    pub location: String,
}

#[derive(Debug)]
pub enum Message {
    NotifyAlive(Alive),
    NotifyByeBye(ByeBye),
    Search(Search),
    Response(Response),
}

pub struct Advertisement {
    pub notification_type: String,
    pub location: url::Url,
}

pub mod ssdp;
pub mod udp;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_debug() {
        println!(
            "{:?}",
            Message::NotifyAlive(Alive {
                notification_type: "".into(),
                unique_service_name: "".into(),
                location: "".into(),
            })
        );
        println!(
            "{:?}",
            Message::NotifyByeBye(ByeBye {
                notification_type: "".into(),
                unique_service_name: "".into(),
            })
        );
        println!(
            "{:?}",
            Message::Search(Search {
                search_target: "".into(),
                maximum_wait_sec: 3,
            })
        );
        println!(
            "{:?}",
            Message::Response(Response {
                search_target: "".into(),
                unique_service_name: "".into(),
                location: "".into(),
            })
        );
    }

    #[test]
    fn can_clone() {
        let _ = Response {
            search_target: "".into(),
            unique_service_name: "".into(),
            location: "".into(),
        }
        .clone();
    }
}
