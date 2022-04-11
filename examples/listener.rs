use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use tokio::try_join;

use bitflags::bitflags;

use std::cell::RefCell;

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

#[derive(Debug)]
pub struct NetworkInterface(u32);

bitflags! {
    pub struct Flags: u32 {
        const NONE = 0;
        const UP = 0x1;
        const BROADCAST = 0x2;
        const LOOPBACK = 0x4;
        const POINTTOPOINT = 0x8; // not preserving Posix misspelling
        const RUNNING = 0x40;
        const PROMISCUOUS = 0x100;
        const MULTICAST = 0x1000;
    }
}

#[derive(Debug)]
pub enum NetworkEvent {
    NewLink(NetworkInterface, String, Flags),
    DelLink(NetworkInterface),
    NewAddr(NetworkInterface, String, IpAddr, u8),
    DelAddr(NetworkInterface),
}

pub struct NetworkInterfaces {
    link_socket: NlSocket,
    addr_socket: NlSocket,
}

impl NetworkInterfaces {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let link_handle = NlSocketHandle::connect(NlFamily::Route, None, &[1])?;
        let link_socket = NlSocket::new(link_handle)?;
        let addr_handle = NlSocketHandle::connect(NlFamily::Route, None, &[5])?;
        let addr_socket = NlSocket::new(addr_handle)?;
        Ok(NetworkInterfaces {
            link_socket,
            addr_socket,
        })
    }

    pub async fn scan(&mut self, func: Box<dyn FnMut(NetworkEvent)>) -> Result<(), Box<dyn Error>> {
        let ifinfomsg = Ifinfomsg::new(
            RtAddrFamily::Unspecified,
            Arphrd::Ether,
            0,
            IffFlags::empty(),
            IffFlags::empty(),
            RtBuffer::new(),
        );
        let nl_header = Nlmsghdr::new(
            None,
            Rtm::Getlink,
            NlmFFlags::new(&[NlmF::Request, NlmF::Match]),
            None,
            None,
            NlPayload::Payload(ifinfomsg),
        );
        self.link_socket.send(&nl_header).await?;

        let ifaddrmsg = Ifaddrmsg {
            ifa_family: RtAddrFamily::Inet,
            ifa_prefixlen: 0,
            ifa_flags: IfaFFlags::empty(),
            ifa_scope: 0,
            ifa_index: 0,
            rtattrs: RtBuffer::new(),
        };
        let nl_header = Nlmsghdr::new(
            None,
            Rtm::Getaddr,
            NlmFFlags::new(&[NlmF::Request, NlmF::Root]),
            None,
            None,
            NlPayload::Payload(ifaddrmsg),
        );
        self.addr_socket.send(&nl_header).await?;

        let f = RefCell::new(func);

        let f1 = NetworkInterfaces::get_links(&mut self.link_socket, &f);
        let f2 = NetworkInterfaces::get_addrs(&mut self.addr_socket, &f);

        try_join!(f1, f2).map(|_| ())
    }

    async fn get_links(
        ss: &mut NlSocket,
        func: &RefCell<Box<dyn FnMut(NetworkEvent)>>,
    ) -> Result<(), Box<dyn Error>> {
        let mut buffer = Vec::new();
        loop {
            let msgs: NlBuffer<Rtm, Ifinfomsg> = ss.recv(&mut buffer).await?;
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
                                newflags = newflags | newf;
                            }
                        }

                        match msg.nl_type {
                            Rtm::Newlink => func.borrow_mut()(NetworkEvent::NewLink(
                                NetworkInterface(p.ifi_index as u32),
                                name,
                                newflags,
                            )),
                            Rtm::Dellink => func.borrow_mut()(NetworkEvent::DelLink(
                                NetworkInterface(p.ifi_index as u32),
                            )),
                            _ => (),
                        }
                    }
                }
            }
        }
    }

    async fn get_addrs(
        ss: &mut NlSocket,
        func: &RefCell<Box<dyn FnMut(NetworkEvent)>>,
    ) -> Result<(), Box<dyn Error>> {
        let mut buffer = Vec::new();
        loop {
            let msgs: NlBuffer<Rtm, Ifaddrmsg> = ss.recv(&mut buffer).await?;
            for msg in msgs {
                if let NlPayload::Payload(p) = msg.nl_payload {
                    let handle = p.rtattrs.get_attr_handle();
                    let addr = {
                        if let Ok(ip_bytes) =
                            handle.get_attr_payload_as_with_len::<&[u8]>(Ifa::Local)
                        {
                            ip(&ip_bytes)
                        } else {
                            None
                        }
                    };
                    let name = handle
                        .get_attr_payload_as_with_len::<String>(Ifa::Label)
                        .ok();
                    if let (Some(addr), Some(name)) = (addr, name) {
                        match msg.nl_type {
                            Rtm::Newaddr => func.borrow_mut()(NetworkEvent::NewAddr(
                                NetworkInterface(p.ifa_index as u32),
                                name,
                                addr,
                                p.ifa_prefixlen,
                            )),
                            Rtm::Deladdr => func.borrow_mut()(NetworkEvent::DelAddr(
                                NetworkInterface(p.ifa_index as u32),
                            )),
                            _ => (),
                        }
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut ni = NetworkInterfaces::new()?;

    ni.scan(Box::new(|ne| println!("{:?}", ne))).await?;

    Ok(())
}
