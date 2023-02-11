use crate::*;
use std::collections::HashMap;

pub fn parse(buf: &[u8]) -> Result<Message, std::io::Error> {
    let packet = std::str::from_utf8(buf)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;

    let mut iter = packet.lines();

    let prefix = iter
        .next()
        .ok_or(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;

    let mut map = HashMap::new();
    for line in iter {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.to_ascii_uppercase(), value.trim());
        }
    }
    match prefix {
        "NOTIFY * HTTP/1.1" => {
            if let Some(&nts) = map.get("NTS") {
                match nts {
                    "ssdp:alive" => {
                        if let (Some(nt), Some(usn), Some(loc)) = (
                            map.get("NT"),
                            map.get("USN"),
                            map.get("LOCATION"),
                        ) {
                            return Ok(Message::NotifyAlive(Alive {
                                notification_type: String::from(*nt),
                                unique_service_name: String::from(*usn),
                                location: String::from(*loc),
                            }));
                        }
                    }
                    "ssdp:byebye" => {
                        if let (Some(nt), Some(usn)) =
                            (map.get("NT"), map.get("USN"))
                        {
                            return Ok(Message::NotifyByeBye(ByeBye {
                                notification_type: String::from(*nt),
                                unique_service_name: String::from(*usn),
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }
        "HTTP/1.1 200 OK" => {
            if let (Some(st), Some(usn), Some(loc)) =
                (map.get("ST"), map.get("USN"), map.get("LOCATION"))
            {
                return Ok(Message::Response(Response {
                    search_target: String::from(*st),
                    unique_service_name: String::from(*usn),
                    location: String::from(*loc),
                }));
            }
        }
        "M-SEARCH * HTTP/1.1" => {
            if let (Some(st), Some(mx)) = (map.get("ST"), map.get("MX")) {
                if let Ok(mxn) = mx.parse::<u8>() {
                    return Ok(Message::Search(Search {
                        search_target: String::from(*st),
                        maximum_wait_sec: mxn,
                    }));
                }
            }
        }
        _ => {}
    }
    Err(std::io::ErrorKind::InvalidData.into())
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::Message;

    #[test]
    fn rejects_non_utf8() {
        assert!(parse(&[128, 128]).is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn rejects_no_crlf() {
        assert!(parse(b"foo-bar").is_err());
    }

    #[test]
    fn rejects_one_crlf() {
        assert!(parse(b"foo-bar\r\nbar-foo").is_err());
    }

    #[test]
    fn rejects_two_crlfs() {
        assert!(parse(b"foo-bar\r\nbar-foo\r\n").is_err());
    }

    #[test]
    fn accepts_hello() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:alive\r\n\
NT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_ok());
        assert!(matches!(r.unwrap(),
                         Message::NotifyAlive(a)
                         if a.notification_type == "fnord"
                         && a.unique_service_name == "prod37"
                         && a.location == "http://foo"));
    }

    #[test]
    fn rejects_notify_bad_nts() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: potato\r\n\
NT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_notify_no_nts() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NXTS: ssdp:alive\r\n\
NT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_hello_no_nt() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:alive\r\n\
XNT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_hello_no_usn() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:alive\r\n\
NT: fnord\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_hello_no_location() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:alive\r\n\
NT: fnord\r\n\
USN: prod37\r\n\
Location\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn accepts_byebye() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:byebye\r\n\
NT: fnord\r\n\
USN: prod37\r\n\
\r\n",
        );
        assert!(r.is_ok());
        assert!(matches!(r.unwrap(),
                         Message::NotifyByeBye(a)
                         if a.notification_type == "fnord"
                         && a.unique_service_name == "prod37"));
    }

    #[test]
    fn rejects_byebye_no_nt() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:byebye\r\n\
XNT: fnord\r\n\
USN: prod37\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_byebye_no_usn() {
        let r = parse(
            b"NOTIFY * HTTP/1.1\r\n\
NTS: ssdp:byebye\r\n\
NT: fnord\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn accepts_response() {
        let r = parse(
            b"HTTP/1.1 200 OK\r\n\
sT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_ok());
        assert!(matches!(r.unwrap(),
                         Message::Response(a)
                         if a.search_target == "fnord"
                         && a.unique_service_name == "prod37"
                         && a.location == "http://foo"));
    }

    #[test]
    fn rejects_response_no_st() {
        let r = parse(
            b"HTTP/1.1 200 OK\r\n\
XsT: fnord\r\n\
USN: prod37\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_response_no_usn() {
        let r = parse(
            b"HTTP/1.1 200 OK\r\n\
sT: fnord\r\n\
Location: http://foo\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn rejects_response_no_location() {
        let r = parse(
            b"HTTP/1.1 200 OK\r\n\
sT: fnord\r\n\
USN: prod37\r\n\
Location\r\n\
\r\n",
        );
        assert!(r.is_err());
    }

    #[test]
    fn accepts_search() {
        let r = parse(b"M-SEARCH * HTTP/1.1\r\nST: foo\r\nMX: 5\r\n\r\n");
        assert!(r.is_ok());
        assert!(matches!(r.unwrap(),
                         Message::Search(s)
                         if s.search_target == "foo"
                         && s.maximum_wait_sec == 5));
    }

    #[test]
    fn rejects_search_no_st() {
        let r = parse(b"M-SEARCH * HTTP/1.1\r\nSXT: foo\r\nMX: 5\r\n\r\n");
        assert!(r.is_err());
    }

    #[test]
    fn rejects_search_no_mx() {
        let r = parse(b"M-SEARCH * HTTP/1.1\r\nST: foo\r\nM: 5\r\n\r\n");
        assert!(r.is_err());
    }

    #[test]
    fn rejects_search_bad_mx() {
        let r = parse(b"M-SEARCH * HTTP/1.1\r\nST: foo\r\nMX: a\r\n\r\n");
        assert!(r.is_err());
    }
}
