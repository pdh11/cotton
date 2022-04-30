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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
