use super::*;

use std::{
    io::Error,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use async_stream::stream;
use futures_util::join;
use futures_util::stream;
use futures_util::Stream;

use neli::{
    consts::{
        nl::{NlmF, NlmFFlags},
        rtnl::{
            Arphrd, Ifa, IfaFFlags, Iff, IffFlags, Ifla, RtAddrFamily, Rtm,
        },
        socket::NlFamily,
    },
    err::DeError,
    err::SerError,
    err::WrappedError,
    nl::{NlPayload, Nlmsghdr},
    rtnl::Ifaddrmsg,
    rtnl::Ifinfomsg,
    socket::tokio::NlSocket,
    socket::NlSocketHandle,
    types::NlBuffer,
    types::RtBuffer,
};

fn ip(ip_bytes: &[u8]) -> Option<IpAddr> {
    match ip_bytes.len() {
        4 => Some(IpAddr::from(Ipv4Addr::from(u32::from_be_bytes(
            ip_bytes.try_into().unwrap(),
        )))),

        16 => Some(IpAddr::from(Ipv6Addr::from(u128::from_be_bytes(
            ip_bytes.try_into().unwrap(),
        )))),

        _ => {
            println!(
                "Unrecognized address length of {} found",
                ip_bytes.len()
            );
            None
        }
    }
}

fn map_rx_error(err: DeError) -> Error {
    if let DeError::Wrapped(WrappedError::IOError(io_error)) = err {
        io_error
    } else {
        Error::from(ErrorKind::Other)
    }
}

fn map_tx_error(err: SerError) -> Error {
    if let SerError::Wrapped(WrappedError::IOError(io_error)) = err {
        io_error
    } else {
        Error::from(ErrorKind::Other)
    }
}

fn map_flags(flags: &IffFlags) -> Flags {
    let mut newflags = Default::default();
    for (iff, newf) in [
        (&Iff::Up, Flags::UP),
        (&Iff::Running, Flags::RUNNING),
        (&Iff::Loopback, Flags::LOOPBACK),
        (&Iff::Pointopoint, Flags::POINTTOPOINT),
        (&Iff::Broadcast, Flags::BROADCAST),
        (&Iff::Multicast, Flags::MULTICAST),
    ] {
        if flags.contains(iff) {
            newflags |= newf;
        }
    }
    newflags
}

fn translate_link_message(
    msg: &Nlmsghdr<Rtm, Ifinfomsg>,
) -> Option<NetworkEvent> {
    if let NlPayload::Payload(p) = &msg.nl_payload {
        match msg.nl_type {
            Rtm::Newlink => {
                let handle = p.rtattrs.get_attr_handle();
                let name = handle
                    .get_attr_payload_as_with_len::<String>(Ifla::Ifname)
                    .ok();
                if let Some(name) = name {
                    let newflags = map_flags(&p.ifi_flags);
                    return Some(NetworkEvent::NewLink(
                        InterfaceIndex(p.ifi_index as u32),
                        name,
                        newflags,
                    ));
                }
            }
            Rtm::Dellink => {
                return Some(NetworkEvent::DelLink(InterfaceIndex(
                    p.ifi_index as u32,
                )))
            }
            _ => (),
        }
    }
    None
}

fn translate_addr_message(
    msg: &Nlmsghdr<Rtm, Ifaddrmsg>,
) -> Option<NetworkEvent> {
    if let NlPayload::Payload(p) = &msg.nl_payload {
        let handle = p.rtattrs.get_attr_handle();
        if let Some(addr) = handle
            .get_attr_payload_as_with_len::<&[u8]>(Ifa::Address)
            .ok()
            .and_then(ip)
        {
            match msg.nl_type {
                Rtm::Newaddr => {
                    return Some(NetworkEvent::NewAddr(
                        InterfaceIndex(p.ifa_index as u32),
                        addr,
                        p.ifa_prefixlen,
                    ))
                }
                Rtm::Deladdr => {
                    return Some(NetworkEvent::DelAddr(
                        InterfaceIndex(p.ifa_index as u32),
                        addr,
                        p.ifa_prefixlen,
                    ))
                }
                _ => (),
            }
        }
    }
    None
}

fn get_links(
    mut ss: NlSocket,
) -> impl Stream<Item = Result<NetworkEvent, Error>> {
    let mut buffer = Vec::new();
    stream! {
        loop {
            let res: Result<NlBuffer<Rtm, Ifinfomsg>, DeError> =
                ss.recv(&mut buffer).await;
            match res {
                Ok(msgs) =>
                    for msg in msgs {
                        if let Some(event) = translate_link_message(&msg) {
                            yield Ok(event);
                        }
                    },
                Err(e) => yield Err(map_rx_error(e))
            }
        }
    }
}

fn get_addrs(
    mut ss: NlSocket,
) -> impl Stream<Item = Result<NetworkEvent, Error>> {
    let mut buffer = Vec::new();
    stream! {
        loop {
            let res: Result<NlBuffer<Rtm, Ifaddrmsg>, DeError> =
                ss.recv(&mut buffer).await;
            match res {
                Ok(msgs) =>
                    for msg in msgs {
                        if let Some(event) = translate_addr_message(&msg) {
                            yield Ok(event);
                        }
                    },
                Err(e) => yield Err(map_rx_error(e))
            }
        }
    }
}

/** Obtain the current list of network interfaces and a stream of future events

The stream consists of a sequence of [NetworkEvent]
objects, each describing a network interface (as
[NetworkEvent::NewLink]) or an address on that interface (as
[NetworkEvent::NewAddr]). An interface may have several addresses,
both IPv4 and IPv6. In all cases, the [NetworkEvent::NewLink] event
describing an interface, will be generated before that interface's
[NetworkEvent::NewAddr] event or events.

All interfaces and addresses already present when get_interfaces_async
is called, will be immediately announced as if newly-added.

If addresses are deactivated or interfaces disappear -- such as when a USB
network adaptor is unplugged -- [NetworkEvent::DelLink]
or [NetworkEvent::DelAddr] events will be generated.

The stream continues to wait for future events, i.e. the `while` loop
in the examples is an *infinite* loop. In normal use, an asynchronous
application would use `tokio::select!` or similar to wait on both
network events from this crate, and the other events specific to that
application.

For a simple listing of the returned information, just use println:

```rust
# use cotton_netif::*;
# use futures_util::StreamExt;
# tokio_test::block_on(async {
let mut s = get_interfaces_async().await?;

while let Some(e) = s.next().await {
    println!("{:?}", e);
#   break;
}
# Ok::<(), std::io::Error>(())
# });
# Ok::<(), std::io::Error>(())
```

As another example, here is how to list all available
multicast-capable interfaces, and be notified if and when new ones
appear:

```rust
# use cotton_netif::*;
# use futures_util::StreamExt;
# tokio_test::block_on(async {
let mut s = get_interfaces_async().await?;

while let Some(e) = s.next().await {
    match e {
        Ok(NetworkEvent::NewLink(_i, name, flags)) => {
            if flags.contains(Flags::RUNNING | Flags::UP | Flags::MULTICAST) {
                println!("New multicast-capable interface: {}", name);
            }
        },
        _ => {},
    }
#   break;
}
# Ok::<(), std::io::Error>(())
# });
# Ok::<(), std::io::Error>(())
```

 */
pub async fn get_interfaces_async(
) -> Result<impl Stream<Item = Result<NetworkEvent, Error>>, Error> {
    /* Group constants from <linux/rtnetlink.h> not wrapped by neli 0.6.1:
     *  1 = RTNLGRP_LINK (link events)
     *  5 = RTNLGRP_IPV4_IFADDR (ipv4 events)
     *  9 = RTNLGRP_IPV6_IFADDR (ipv6 events)
     */
    let link_handle = NlSocketHandle::connect(NlFamily::Route, None, &[1])?;
    let mut link_socket = NlSocket::new(link_handle)?;
    let addr4_handle = NlSocketHandle::connect(NlFamily::Route, None, &[5])?;
    let mut addr4_socket = NlSocket::new(addr4_handle)?;
    let addr6_handle = NlSocketHandle::connect(NlFamily::Route, None, &[9])?;
    let mut addr6_socket = NlSocket::new(addr6_handle)?;

    let ifinfomsg = Ifinfomsg::new(
        RtAddrFamily::Unspecified,
        Arphrd::Ether,
        0,
        IffFlags::empty(),
        IffFlags::empty(),
        RtBuffer::new(),
    );
    let nl_link_header = Nlmsghdr::new(
        None,
        Rtm::Getlink,
        NlmFFlags::new(&[NlmF::Request, NlmF::Match]),
        None,
        None,
        NlPayload::Payload(ifinfomsg),
    );

    let ifaddrmsg = Ifaddrmsg {
        ifa_family: RtAddrFamily::Inet,
        ifa_prefixlen: 0,
        ifa_flags: IfaFFlags::empty(),
        ifa_scope: 0,
        ifa_index: 0,
        rtattrs: RtBuffer::new(),
    };
    let nl_addr4_header = Nlmsghdr::new(
        None,
        Rtm::Getaddr,
        NlmFFlags::new(&[NlmF::Request, NlmF::Root]),
        None,
        None,
        NlPayload::Payload(ifaddrmsg),
    );

    let ifaddr6msg = Ifaddrmsg {
        ifa_family: RtAddrFamily::Inet6,
        ifa_prefixlen: 0,
        ifa_flags: IfaFFlags::empty(),
        ifa_scope: 0,
        ifa_index: 0,
        rtattrs: RtBuffer::new(),
    };
    let nl_addr6_header = Nlmsghdr::new(
        None,
        Rtm::Getaddr,
        NlmFFlags::new(&[NlmF::Request, NlmF::Root]),
        None,
        None,
        NlPayload::Payload(ifaddr6msg),
    );

    let (rc1, rc2, rc3) = join! {
        link_socket.send(&nl_link_header),
        addr4_socket.send(&nl_addr4_header),
        addr6_socket.send(&nl_addr6_header),
    };
    rc1.and(rc2).and(rc3).map_err(map_tx_error)?;

    Ok(stream::select(
        Box::pin(get_links(link_socket)),
        stream::select(
            Box::pin(get_addrs(addr4_socket)),
            Box::pin(get_addrs(addr6_socket)),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use neli::rtnl::Rtattr;
    use neli::ToBytes;
    use std::os::unix::io::FromRawFd;
    use tokio_test::block_on;

    #[test]
    fn parse_4byte_addr() {
        let input = [192u8, 168u8, 0u8, 200u8];

        let result = ip(&input);

        assert_eq!(result, "192.168.0.200".parse().ok());
    }

    #[test]
    fn parse_16byte_addr() {
        let input = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];

        let result = ip(&input);

        assert_eq!(result, "::1".parse().ok());
    }

    #[test]
    fn no_parse_5byte_addr() {
        let input = [2u8, 3, 4, 5, 6];

        let result = ip(&input);

        assert_eq!(result, None);
    }

    #[test]
    fn test_rx_io_error_mapped() {
        let err = map_rx_error(DeError::Wrapped(WrappedError::IOError(
            std::io::Error::from(ErrorKind::UnexpectedEof),
        )));
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_rx_io_error_not_mapped() {
        let err = map_rx_error(DeError::BufferNotParsed);
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[test]
    fn test_tx_io_error_mapped() {
        let err = map_tx_error(SerError::Wrapped(WrappedError::IOError(
            std::io::Error::from(ErrorKind::UnexpectedEof),
        )));
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_tx_io_error_not_mapped() {
        let err = map_tx_error(SerError::BufferNotFilled);
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[test]
    fn test_map_up() {
        assert_eq!(map_flags(&IffFlags::new(&[Iff::Up])), Flags::UP);
    }

    #[test]
    fn test_map_running() {
        assert_eq!(map_flags(&IffFlags::new(&[Iff::Running])), Flags::RUNNING);
    }

    #[test]
    fn test_map_loopback() {
        assert_eq!(
            map_flags(&IffFlags::new(&[Iff::Loopback])),
            Flags::LOOPBACK
        );
    }

    #[test]
    fn test_map_pointtopoint() {
        assert_eq!(
            map_flags(&IffFlags::new(&[Iff::Pointopoint])),
            Flags::POINTTOPOINT
        );
    }

    #[test]
    fn test_map_broadcast() {
        assert_eq!(
            map_flags(&IffFlags::new(&[Iff::Broadcast])),
            Flags::BROADCAST
        );
    }

    #[test]
    fn test_map_multicast() {
        assert_eq!(
            map_flags(&IffFlags::new(&[Iff::Multicast])),
            Flags::MULTICAST
        );
    }

    #[test]
    fn test_map_several() {
        assert_eq!(
            map_flags(&IffFlags::new(&[
                Iff::Up,
                Iff::Running,
                Iff::Multicast
            ])),
            Flags::UP | Flags::RUNNING | Flags::MULTICAST
        );
    }

    #[test]
    fn test_link_message_no_payload() {
        let msg = Nlmsghdr::new(
            None,
            Rtm::Getlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Empty,
        );

        assert!(translate_link_message(&msg).is_none());
    }

    #[test]
    fn test_link_message_no_name() {
        let msg = Nlmsghdr::new(
            None,
            Rtm::Newlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifinfomsg::new(
                RtAddrFamily::Inet,
                Arphrd::Ether,
                0,
                IffFlags::empty(),
                IffFlags::empty(),
                RtBuffer::new(),
            )),
        );

        assert!(translate_link_message(&msg).is_none());
    }

    #[test]
    fn test_link_message_no_type() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifla::Ifname, "eth0".to_string()).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Getlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifinfomsg::new(
                RtAddrFamily::Inet,
                Arphrd::Ether,
                0,
                IffFlags::empty(),
                IffFlags::empty(),
                buf,
            )),
        );

        assert!(translate_link_message(&msg).is_none());
    }

    #[test]
    fn test_link_message_new() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifla::Ifname, "eth0".to_string()).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Newlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifinfomsg::new(
                RtAddrFamily::Inet,
                Arphrd::Ether,
                3,
                IffFlags::empty(),
                IffFlags::empty(),
                buf,
            )),
        );

        let event = translate_link_message(&msg);
        assert!(event.is_some());
        assert_eq!(
            event.unwrap(),
            NetworkEvent::NewLink(
                InterfaceIndex(3),
                "eth0".to_string(),
                Default::default()
            )
        );
    }

    #[test]
    fn test_link_message_del() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifla::Ifname, "eth1".to_string()).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Dellink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifinfomsg::new(
                RtAddrFamily::Inet,
                Arphrd::Ether,
                2,
                IffFlags::empty(),
                IffFlags::empty(),
                buf,
            )),
        );

        let event = translate_link_message(&msg);
        assert!(event.is_some());
        assert_eq!(event.unwrap(), NetworkEvent::DelLink(InterfaceIndex(2)));
    }

    #[test]
    fn test_addr_message_no_payload() {
        let msg = Nlmsghdr::new(
            None,
            Rtm::Getlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Empty,
        );

        assert!(translate_addr_message(&msg).is_none());
    }

    #[test]
    fn test_addr_message_no_addr() {
        let msg = Nlmsghdr::new(
            None,
            Rtm::Newlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 0,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: RtBuffer::new(),
            }),
        );

        assert!(translate_addr_message(&msg).is_none());
    }

    #[test]
    fn test_addr_message_bad_addr() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifa::Address, 65535u16).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Newlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 0,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: buf,
            }),
        );

        assert!(translate_addr_message(&msg).is_none());
    }

    #[test]
    fn test_addr_message_bad_type() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifa::Address, 65535u32).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Newlink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 0,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: buf,
            }),
        );

        assert!(translate_addr_message(&msg).is_none());
    }

    #[test]
    fn test_addr_message_new() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifa::Address, 65535u32).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Newaddr,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 24,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: buf,
            }),
        );

        let event = translate_addr_message(&msg);
        assert!(event.is_some());
        assert_eq!(
            event.unwrap(),
            NetworkEvent::NewAddr(
                InterfaceIndex(2),
                ip(&[255, 255, 0, 0]).unwrap(),
                24
            )
        );
    }

    #[test]
    fn test_addr_message_del() {
        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifa::Address, 65535u32).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Deladdr,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 24,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: buf,
            }),
        );

        let event = translate_addr_message(&msg);
        assert!(event.is_some());
        assert_eq!(
            event.unwrap(),
            NetworkEvent::DelAddr(
                InterfaceIndex(2),
                ip(&[255, 255, 0, 0]).unwrap(),
                24
            )
        );
    }

    #[tokio::test]
    async fn get_links_bad_message() {
        let (infd, outfd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Datagram,
            None,
            nix::sys::socket::SockFlag::empty(),
        )
        .unwrap();

        let nlsocket = unsafe {
            NlSocket::new(NlSocketHandle::from_raw_fd(outfd)).unwrap()
        };

        nix::sys::socket::sendto(
            infd,
            &[1, 2, 3, 4, 5],
            &(),
            nix::sys::socket::MsgFlags::empty(),
        )
        .unwrap();

        let s = Box::pin(get_links(nlsocket)).next().await;
        assert!(s.is_some());
        let result = s.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_links_del() {
        let (infd, outfd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Datagram,
            None,
            nix::sys::socket::SockFlag::empty(),
        )
        .unwrap();

        let nlsocket = unsafe {
            NlSocket::new(NlSocketHandle::from_raw_fd(outfd)).unwrap()
        };

        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifla::Ifname, "eth1".to_string()).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Dellink,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifinfomsg::new(
                RtAddrFamily::Inet,
                Arphrd::Ether,
                2,
                IffFlags::empty(),
                IffFlags::empty(),
                buf,
            )),
        );

        let mut v = std::io::Cursor::new(Vec::new());
        msg.to_bytes(&mut v).unwrap();

        nix::sys::socket::sendto(
            infd,
            &v.into_inner(),
            &(),
            nix::sys::socket::MsgFlags::empty(),
        )
        .unwrap();

        let s = Box::pin(get_links(nlsocket)).next().await;

        assert!(s.is_some());
        let result = s.unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NetworkEvent::DelLink(InterfaceIndex(2)));
    }

    #[tokio::test]
    async fn get_addrs_bad_message() {
        let (infd, outfd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Datagram,
            None,
            nix::sys::socket::SockFlag::empty(),
        )
        .unwrap();

        let nlsocket = unsafe {
            NlSocket::new(NlSocketHandle::from_raw_fd(outfd)).unwrap()
        };

        nix::sys::socket::sendto(
            infd,
            &[1, 2, 3, 4, 5],
            &(),
            nix::sys::socket::MsgFlags::empty(),
        )
        .unwrap();

        let s = Box::pin(get_addrs(nlsocket)).next().await;
        assert!(s.is_some());
        let result = s.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_addr_del() {
        let (infd, outfd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Datagram,
            None,
            nix::sys::socket::SockFlag::empty(),
        )
        .unwrap();

        let nlsocket = unsafe {
            NlSocket::new(NlSocketHandle::from_raw_fd(outfd)).unwrap()
        };

        let mut buf = RtBuffer::new();
        buf.push(Rtattr::new(None, Ifa::Address, 65535u32).unwrap());

        let msg = Nlmsghdr::new(
            None,
            Rtm::Deladdr,
            NlmFFlags::empty(),
            None,
            None,
            NlPayload::Payload(Ifaddrmsg {
                ifa_family: RtAddrFamily::Inet,
                ifa_prefixlen: 24,
                ifa_flags: IfaFFlags::empty(),
                ifa_scope: 0,
                ifa_index: 2,
                rtattrs: buf,
            }),
        );

        let mut v = std::io::Cursor::new(Vec::new());
        msg.to_bytes(&mut v).unwrap();

        nix::sys::socket::sendto(
            infd,
            &v.into_inner(),
            &(),
            nix::sys::socket::MsgFlags::empty(),
        )
        .unwrap();

        let s = Box::pin(get_addrs(nlsocket)).next().await;

        assert!(s.is_some());
        let event = s.unwrap();
        assert!(event.is_ok());
        assert_eq!(
            event.unwrap(),
            NetworkEvent::DelAddr(
                InterfaceIndex(2),
                ip(&[255, 255, 0, 0]).unwrap(),
                24
            )
        );
    }

    #[test]
    fn zzz_instantiate() {
        assert!(block_on(get_interfaces_async()).is_ok());
    }
}
