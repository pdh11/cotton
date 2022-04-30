use super::*;

use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use async_stream::stream;
use futures_util::stream;
use futures_util::Stream;

use neli::{
    consts::{
        nl::{NlmF, NlmFFlags},
        rtnl::{Arphrd, Ifa, IfaFFlags, Iff, IffFlags, Ifla, RtAddrFamily, Rtm},
        socket::NlFamily,
    },
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
        4 => Some(IpAddr::from(Ipv4Addr::from(
            u32::from_ne_bytes(ip_bytes.try_into().unwrap()).to_be(),
        ))),

        16 => Some(IpAddr::from(Ipv6Addr::from(
            u128::from_ne_bytes(ip_bytes.try_into().unwrap()).to_be(),
        ))),

        _ => {
            println!("Unrecognized address length of {} found", ip_bytes.len());
            None
        }
    }
}

fn get_links(mut ss: NlSocket) -> impl Stream<Item = NetworkEvent> {
    let mut buffer = Vec::new();
    stream! {
        loop {
            let msgs: NlBuffer<Rtm, Ifinfomsg> =
                ss.recv(&mut buffer).await.unwrap();
            for msg in msgs {
                if let NlPayload::Payload(p) = msg.nl_payload {
                    let handle = p.rtattrs.get_attr_handle();
                    let name = handle
                        .get_attr_payload_as_with_len::<String>(Ifla::Ifname)
                        .ok();
                    if let Some(name) = name {
                        let flags = p.ifi_flags;
                        let mut newflags = Flags::NONE;
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
                        match msg.nl_type {
                            Rtm::Newlink => yield NetworkEvent::NewLink(
                                NetworkInterface(p.ifi_index as u32),
                                name,
                                newflags,
                            ),
                            Rtm::Dellink => yield NetworkEvent::DelLink(
                                NetworkInterface(p.ifi_index as u32),
                            ),
                            _ => (),
                        }
                    }
                }
            }
        }
    }
}

fn get_addrs(mut ss: NlSocket) -> impl Stream<Item = NetworkEvent> {
    let mut buffer = Vec::new();
    stream! {
        loop {
            let msgs: NlBuffer<Rtm, Ifaddrmsg> = ss.recv(&mut buffer).await.unwrap();
            for msg in msgs {
                if let NlPayload::Payload(p) = msg.nl_payload {
                    let handle = p.rtattrs.get_attr_handle();
                    let addr = {
                        if let Ok(ip_bytes) =
                            handle.get_attr_payload_as_with_len::<&[u8]>(Ifa::Local)
                        {
                            ip(ip_bytes)
                        } else {
                            None
                        }
                    };
                    let name = handle
                        .get_attr_payload_as_with_len::<String>(Ifa::Label)
                        .ok();
                    if let (Some(addr), Some(name)) = (addr, name) {
                        match msg.nl_type {
                            Rtm::Newaddr => yield NetworkEvent::NewAddr(
                                NetworkInterface(p.ifa_index as u32),
                                name,
                                addr,
                                p.ifa_prefixlen,
                            ),
                            Rtm::Deladdr => yield NetworkEvent::DelAddr(
                                NetworkInterface(p.ifa_index as u32),
                            ),
                            _ => (),
                        }
                    }
                }
            }
        }
    }
}

pub async fn network_interfaces_dynamic() -> Result<impl Stream<Item = NetworkEvent>, Box<dyn Error>>
{
    /* Group constants from <linux/rtnetlink.h> not wrapped by neli 0.6.1:
     *  1 = RTNLGRP_LINK (link events)
     *  5 = RTNLGRP_IPV4_IFADDR (ipv4 events)
     *  9 = RTNLGRP_IPV6_IFADDR (ipv6 events)
     */
    let link_handle = NlSocketHandle::connect(NlFamily::Route, None, &[1])?;
    let mut link_socket = NlSocket::new(link_handle)?;
    let addr_handle = NlSocketHandle::connect(NlFamily::Route, None, &[5, 9])?;
    let mut addr_socket = NlSocket::new(addr_handle)?;

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

    link_socket.send(&nl_link_header).await?;

    let ifaddrmsg = Ifaddrmsg {
        ifa_family: RtAddrFamily::Inet,
        ifa_prefixlen: 0,
        ifa_flags: IfaFFlags::empty(),
        ifa_scope: 0,
        ifa_index: 0,
        rtattrs: RtBuffer::new(),
    };
    let nl_addr_header = Nlmsghdr::new(
        None,
        Rtm::Getaddr,
        NlmFFlags::new(&[NlmF::Request, NlmF::Root]),
        None,
        None,
        NlPayload::Payload(ifaddrmsg),
    );

    addr_socket.send(&nl_addr_header).await?;

    Ok(stream::select(
        Box::pin(get_links(link_socket)),
        Box::pin(get_addrs(addr_socket)),
    ))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn parse_4byte_addr() {
        let input = [192u8, 168u8, 0u8, 200u8];

        let result = ip(&input);

        assert_eq!(result, "192.168.0.200".parse().ok());
    }

    #[test]
    fn parse_16byte_addr() {
        let input = [0u8, 0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,1];

        let result = ip(&input);

        assert_eq!(result, "::1".parse().ok());
    }

    #[test]
    fn no_parse_5byte_addr() {
        let input = [2u8, 3, 4, 5, 6];

        let result = ip(&input);

        assert_eq!(result, None);
    }
}
