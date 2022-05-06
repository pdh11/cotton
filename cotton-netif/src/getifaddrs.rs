use super::*;
use nix::ifaddrs;
use nix::net::if_::InterfaceFlags;

use std::{
    io::Error,
    net::{IpAddr, Ipv4Addr},
};

use async_stream::stream;
use futures_util::Stream;

pub async fn network_interfaces_static(
) -> Result<impl Stream<Item = Result<NetworkEvent, Error>>, Error> {
    let addrs = ifaddrs::getifaddrs()?;

    Ok(Box::pin(stream! {
        let mut ix = 0u32;
        for ifaddr in addrs {
            //println!("static {:?}", ifaddr);
            if let (Some(addr), Some(mask)) = (ifaddr.address, ifaddr.netmask) {
                let flags = ifaddr.flags;
                let mut newflags = Flags::NONE;
                for (iff, newf) in [
                    (InterfaceFlags::IFF_UP, Flags::UP),
                    (InterfaceFlags::IFF_RUNNING, Flags::RUNNING),
                    (InterfaceFlags::IFF_LOOPBACK, Flags::LOOPBACK),
                    (InterfaceFlags::IFF_POINTOPOINT, Flags::POINTTOPOINT),
                    (InterfaceFlags::IFF_BROADCAST, Flags::BROADCAST),
                    (InterfaceFlags::IFF_MULTICAST, Flags::MULTICAST),
                ] {
                    if flags.contains(iff) {
                        newflags |= newf;
                    }
                }

                yield Ok(NetworkEvent::NewLink(
                    InterfaceIndex(ix),
                    ifaddr.interface_name,
                    newflags));

                if let Some(ipv4) = addr.as_sockaddr_in() {
                    let ip = IpAddr::from(Ipv4Addr::from(ipv4.ip()));
                    if let Some(netmask) = mask.as_sockaddr_in() {
                        yield Ok(NetworkEvent::NewAddr(
                            InterfaceIndex(ix),
                            ip, netmask.ip().leading_ones() as u8));
                    }

                } else if let Some(_ipv6) = addr.as_sockaddr_in6() {
                    /* @todo -- link up InterfaceIndex with earlier IPv4
                     * version of interface
                     *
                    yield Ok(NetworkEvent::NewAddr(
                        InterfaceIndex(ix),
                        IpAddr::from(ipv6.ip()), 0u8));
                     */
                }
                ix += 1;
            }
        }
    }))
}
