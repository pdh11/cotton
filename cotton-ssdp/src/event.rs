#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Incoming SSDP notification, obtained from
/// [`Service::subscribe`](crate::Service::subscribe)
///
/// Sent in response to searches, and when new resources are made
/// available, and periodically otherwise just in case.
///
/// SSDP notification messages are sent both on arrival and on
/// departure of network resources. Arrivals are distinguished from
/// departures by the notification subtype: either "Alive" or
/// "Bye-bye".
///
/// Only notifications of alive (arriving) resources have a Location
/// field, so this is expressed in the enum.
///
/// Neither [`Service`](crate::Service) nor
/// [`AsyncService`](crate::AsyncService) de-duplicates these
/// notifications -- in other words, a caller of
/// [`Service::subscribe`](crate::Service::subscribe) is likely to
/// receive multiple copies of each. The `unique_service_name` field
/// can be used to distinguish genuinely new resources (e.g., as the
/// key in a `HashMap`).
///
#[derive(Debug, Clone)]
pub enum Notification {
    /// The resource in question is now active (at this location/URL)
    Alive {
        /// Resource type, e.g. "urn:schemas-upnp-org:service:ContentDirectory:1"
        notification_type: String,

        /// Unique identifier for this particular resource instance
        unique_service_name: String,

        /// URL of the resource (for UPnP, the device description document)
        location: String,
    },

    /// The resource in question is (becoming) inactive
    ByeBye {
        /// Resource type
        notification_type: String,

        /// Unique identifier for this particular resource instance
        unique_service_name: String,
    },
}

/// Outgoing SSDP announcement, passed to
/// [`Service::advertise`](crate::Service::advertise)
pub struct Advertisement {
    /// Resource type
    pub notification_type: String,

    /// Resource location
    pub location: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    fn can_debug() {
        let e = format!(
            "{:?}",
            Notification::Alive {
                notification_type: String::new(),
                unique_service_name: String::new(),
                location: String::new(),
            }
        );
        assert_eq!(e, "Alive { notification_type: \"\", unique_service_name: \"\", location: \"\" }".to_string());
    }

    #[test]
    #[allow(clippy::redundant_clone)]
    fn can_clone() {
        let _ = Notification::Alive {
            notification_type: String::new(),
            unique_service_name: String::new(),
            location: String::new(),
        }
        .clone();
    }
}
