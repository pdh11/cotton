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
