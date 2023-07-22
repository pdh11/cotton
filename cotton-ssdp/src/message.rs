extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use core::fmt::Write;

#[derive(Debug)]
pub enum Message {
    NotifyAlive {
        notification_type: String,
        unique_service_name: String,
        location: String,
    },
    NotifyByeBye {
        notification_type: String,
        unique_service_name: String,
    },
    Search {
        search_target: String,
        maximum_wait_sec: u8,
    },
    Response {
        search_target: String,
        unique_service_name: String,
        location: String,
    },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidData,
    UnexpectedEof,
}

pub fn parse(buf: &[u8]) -> Result<Message, Error> {
    let packet = core::str::from_utf8(buf).map_err(|_| Error::InvalidData)?;

    let mut iter = packet.lines();

    let prefix = iter.next().ok_or(Error::UnexpectedEof)?;

    let mut map = BTreeMap::new();
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
                            return Ok(Message::NotifyAlive {
                                notification_type: String::from(*nt),
                                unique_service_name: String::from(*usn),
                                location: String::from(*loc),
                            });
                        }
                    }
                    "ssdp:byebye" => {
                        if let (Some(nt), Some(usn)) =
                            (map.get("NT"), map.get("USN"))
                        {
                            return Ok(Message::NotifyByeBye {
                                notification_type: String::from(*nt),
                                unique_service_name: String::from(*usn),
                            });
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
                return Ok(Message::Response {
                    search_target: String::from(*st),
                    unique_service_name: String::from(*usn),
                    location: String::from(*loc),
                });
            }
        }
        "M-SEARCH * HTTP/1.1" => {
            if let (Some(st), Some(mx)) = (map.get("ST"), map.get("MX")) {
                if let Ok(mxn) = mx.parse::<u8>() {
                    return Ok(Message::Search {
                        search_target: String::from(*st),
                        maximum_wait_sec: mxn,
                    });
                }
            }
        }
        _ => {}
    }
    Err(Error::InvalidData)
}

/// A replacement for Cursor that works in `no_std`
struct MessageCursor<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> MessageCursor<'a> {
    pub fn new(buf: &'a mut [u8]) -> MessageCursor {
        MessageCursor { buf, offset: 0 }
    }

    pub const fn position(&self) -> usize {
        self.offset
    }
}

impl<'a> core::fmt::Write for MessageCursor<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len();
        if n + self.offset > self.buf.len() {
            return core::fmt::Result::Err(core::fmt::Error);
        }
        let b = s.as_bytes();
        self.buf[self.offset..self.offset + n].clone_from_slice(b);
        self.offset += n;
        core::fmt::Result::Ok(())
    }
}

#[allow(clippy::cast_possible_truncation)]
pub fn build_search(buf: &mut [u8], search_type: &str) -> usize {
    let mut cursor = MessageCursor::new(buf);
    let _ = write!(
        cursor,
        "M-SEARCH * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
MAN: \"ssdp:discover\"\r
MX: 5\r
ST: {search_type}\r
\r\n"
    );
    cursor.position()
}

#[allow(clippy::cast_possible_truncation)]
pub fn build_response(
    buf: &mut [u8],
    search_target: &str,
    unique_service_name: &str,
    location: &str,
) -> usize {
    let mut cursor = MessageCursor::new(buf);
    let _ = write!(
        cursor,
        "HTTP/1.1 200 OK\r
CACHE-CONTROL: max-age=1800\r
ST: {search_target}\r
USN: {unique_service_name}\r
LOCATION: {location}\r
SERVER: none/0 UPnP/1.0 {}/{}\r
\r\n",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );
    cursor.position()
}

#[allow(clippy::cast_possible_truncation)]
pub fn build_notify(
    buf: &mut [u8],
    notification_type: &str,
    unique_service_name: &str,
    location: &str,
) -> usize {
    let mut cursor = MessageCursor::new(buf);
    let _ = write!(
        cursor,
        "NOTIFY * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
CACHE-CONTROL: max-age=1800\r
LOCATION: {}\r
NT: {}\r
NTS: ssdp:alive\r
USN: {}\r
SERVER: none/0 UPnP/1.0 {}/{}\r
\r\n",
        location,
        notification_type,
        unique_service_name,
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );
    cursor.position()
}

#[allow(clippy::cast_possible_truncation)]
pub fn build_byebye(
    buf: &mut [u8],
    notification_type: &str,
    unique_service_name: &str,
) -> usize {
    let mut cursor = MessageCursor::new(buf);
    let _ = write!(
        cursor,
        "NOTIFY * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
CACHE-CONTROL: max-age=1800\r
NT: {}\r
NTS: ssdp:byebye\r
USN: {}\r
SERVER: none/0 UPnP/1.0 {}/{}\r
\r\n",
        notification_type,
        unique_service_name,
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );
    cursor.position()
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
            Message::NotifyAlive {
                notification_type: String::new(),
                unique_service_name: String::new(),
                location: String::new(),
            }
        );
        assert_eq!(e, "NotifyAlive { notification_type: \"\", unique_service_name: \"\", location: \"\" }".to_string());

        let e = format!(
            "{:?}",
            Message::NotifyByeBye {
                notification_type: String::new(),
                unique_service_name: String::new(),
            }
        );
        assert_eq!(e, "NotifyByeBye { notification_type: \"\", unique_service_name: \"\" }".to_string());

        let e = format!(
            "{:?}",
            Message::Search {
                search_target: String::new(),
                maximum_wait_sec: 3,
            }
        );
        assert_eq!(
            e,
            "Search { search_target: \"\", maximum_wait_sec: 3 }".to_string()
        );

        let e = format!(
            "{:?}",
            Message::Response {
                search_target: String::new(),
                unique_service_name: String::new(),
                location: String::new(),
            }
        );
        assert_eq!(e, "Response { search_target: \"\", unique_service_name: \"\", location: \"\" }".to_string());
    }

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
                         Message::NotifyAlive {notification_type, unique_service_name, location}
                         if notification_type == "fnord"
                         && unique_service_name == "prod37"
                         && location == "http://foo"));
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
                         Message::NotifyByeBye { notification_type, unique_service_name }
                         if notification_type == "fnord"
                         && unique_service_name == "prod37"));
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
                         Message::Response { search_target, unique_service_name, location }
                         if search_target == "fnord"
                         && unique_service_name == "prod37"
                         && location == "http://foo"));
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
                         Message::Search { search_target, maximum_wait_sec }
                         if search_target == "foo"
                         && maximum_wait_sec == 5));
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

    #[test]
    fn builds_search() {
        let mut buf = [0u8; 512];

        let n = build_search(&mut buf, "upnp::rootdevice");

        let expected = b"M-SEARCH * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
MAN: \"ssdp:discover\"\r
MX: 5\r
ST: upnp::rootdevice\r
\r\n";
        assert!(expected.len() == n);
        assert!(expected[0..n] == buf[0..n]);
    }

    #[test]
    fn builds_response() {
        let mut buf = [0u8; 512];

        let n = build_response(
            &mut buf,
            "upnp::rootdevice",
            "uuid:37",
            "http://me",
        );
        let expected = format!(
            "HTTP/1.1 200 OK\r
CACHE-CONTROL: max-age=1800\r
ST: upnp::rootdevice\r
USN: uuid:37\r
LOCATION: http://me\r
SERVER: none/0 UPnP/1.0 {}/{}\r
\r\n",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        );
        assert!(expected.len() == n);
        assert!(expected.as_bytes()[0..n] == buf[0..n]);
    }

    #[test]
    fn builds_notify() {
        let mut buf = [0u8; 512];

        let n =
            build_notify(&mut buf, "upnp::rootdevice", "uuid:37", "http://me");
        let expected = format!(
            "NOTIFY * HTTP/1.1\r
HOST: 239.255.255.250:1900\r
CACHE-CONTROL: max-age=1800\r
LOCATION: http://me\r
NT: upnp::rootdevice\r
NTS: ssdp:alive\r
USN: uuid:37\r
SERVER: none/0 UPnP/1.0 {}/{}\r
\r\n",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        );
        assert!(expected.len() == n);
        assert!(expected.as_bytes()[0..n] == buf[0..n]);
    }

    #[test]
    fn search_round_trip() {
        let mut buf = [0u8; 512];
        let n = build_search(&mut buf, "upnp::rootdevice");
        let msg = parse(&buf[0..n]).unwrap();
        assert!(matches!(msg,
                         Message::Search { search_target, maximum_wait_sec }
                         if search_target == "upnp::rootdevice"
                         && maximum_wait_sec == 5));
    }

    #[test]
    fn response_round_trip() {
        let mut buf = [0u8; 512];
        let n = build_response(
            &mut buf,
            "upnp::rootdevice",
            "uuid:xyz",
            "https://you",
        );
        let msg = parse(&buf[0..n]).unwrap();
        assert!(matches!(msg,
                         Message::Response { search_target, unique_service_name, location }
                         if search_target == "upnp::rootdevice"
                         && unique_service_name == "uuid:xyz"
                         && location == "https://you"));
    }

    #[test]
    fn notify_round_trip() {
        let mut buf = [0u8; 512];
        let n = build_notify(
            &mut buf,
            "upnp::rootdevice",
            "uuid:xyz",
            "https://you",
        );
        let msg = parse(&buf[0..n]).unwrap();
        assert!(matches!(msg,
                         Message::NotifyAlive { notification_type, unique_service_name, location }
                         if notification_type == "upnp::rootdevice"
                         && unique_service_name == "uuid:xyz"
                         && location == "https://you"));
    }

    #[test]
    fn byebye_round_trip() {
        let mut buf = [0u8; 512];
        let n = build_byebye(&mut buf, "upnp::rootdevice", "uuid:xyz");
        let msg = parse(&buf[0..n]).unwrap();
        assert!(matches!(msg,
                         Message::NotifyByeBye { notification_type, unique_service_name }
                         if notification_type == "upnp::rootdevice"
                         && unique_service_name == "uuid:xyz"));
    }

    #[test]
    fn display_error() {
        let e = Error::InvalidData;
        let e = format!("{e:?}");
        assert_eq!(e, "InvalidData".to_string());
    }

    #[test]
    fn overflow() {
        let mut buf = [0u8; 6];
        let e = build_response(&mut buf, "foo", "bar", "wurdle");
        assert!(e <= 6);
    }
}
