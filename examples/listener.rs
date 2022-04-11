use std::{
    collections::HashMap,
    error::Error,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use tokio::try_join;

use neli::{
    attr::Attribute,
    consts::{
        nl::{NlmF, NlmFFlags},
        rtnl::{Iff, Ifa, Ifla, IfaFFlags, RtAddrFamily, RtScope, Rtm, IffFlags, Arphrd},
        socket::NlFamily,
    },
    err::NlError,
    nl::{NlPayload, Nlmsghdr},
    socket::tokio::NlSocket,
    rtnl::Ifaddrmsg,
    rtnl::Ifinfomsg,
    socket::NlSocketHandle,
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
/*
msg=Nlmsghdr
{ nl_len: 88, nl_type: Newaddr,
  nl_flags: NlmFFlags(FlagBuffer(2, PhantomData)),
  nl_seq: 0, nl_pid: 476654,
  nl_payload: Payload(
      Ifaddrmsg
      { ifa_family: Inet, ifa_prefixlen: 24,
        ifa_flags: IfaFFlags(FlagBuffer(128, PhantomData)), ifa_scope: 0, ifa_index: 4,
        rtattrs: RtBuffer(
            [
                Rtattr { rta_len: 8, rta_type: Address, rta_payload: Buffer },
                Rtattr { rta_len: 8, rta_type: Local, rta_payload: Buffer },
                Rtattr { rta_len: 8, rta_type: Broadcast, rta_payload: Buffer },
                Rtattr { rta_len: 9, rta_type: Label, rta_payload: Buffer },
                Rtattr { rta_len: 8, rta_type: Flags, rta_payload: Buffer },
                Rtattr { rta_len: 20, rta_type: Cacheinfo, rta_payload: Buffer }]) }) }
*/
fn handle(msg: Nlmsghdr<Rtm, Ifaddrmsg>) {
    //println!("msg={:?}", msg);
    if let NlPayload::Payload(p) = msg.nl_payload {
        let handle = p.rtattrs.get_attr_handle();
        let addr = {
            if let Ok(mut ip_bytes) = handle.get_attr_payload_as_with_len::<&[u8]>(Ifa::Local) {
                ip(&ip_bytes)
            } else {
                None
            }
        };
        let bcast = {
            if let Ok(mut ip_bytes) = handle.get_attr_payload_as_with_len::<&[u8]>(Ifa::Broadcast) {
                ip(&ip_bytes)
            } else {
                None
            }
        };
        let flags = {
            if let Ok(mut flag_bytes) = handle.get_attr_payload_as_with_len::<&[u8]>(Ifa::Flags) {
                if let Ok(f) = flag_bytes.try_into() {
                    u32::from_ne_bytes(f)
                } else {
                    0
                }
            } else {
                0
            }
        };
        let name = handle
            .get_attr_payload_as_with_len::<String>(Ifa::Label)
            .ok();
        if let (Some(addr), Some(name)) = (addr, name) {
            print!("{}", match msg.nl_type {
                Rtm::Newaddr => "NEWADDR",
                Rtm::Deladdr => "DELADDR",
                _ => "???ADDR"});
            println!(" {}: {} {}/{} {:x} {:?}", p.ifa_index, name, addr,
                     p.ifa_prefixlen, flags, bcast);
        }
    }
}

fn handle_link(msg: Nlmsghdr<Rtm, Ifinfomsg>) {
    if let NlPayload::Payload(p) = msg.nl_payload {
        let handle = p.rtattrs.get_attr_handle();
        /*
        let flags = {
            if let Ok(mut flag_bytes) = handle.get_attr_payload_as_with_len::<&[u8]>(Ifla::Flags) {
                if let Ok(f) = flag_bytes.try_into() {
                    u32::from_ne_bytes(f)
                } else {
                    0
                }
            } else {
                0
            }
    };*/
        let flags = p.ifi_flags;
        let name = handle
            .get_attr_payload_as_with_len::<String>(Ifla::Ifname)
            .ok();
        if let Some(name) = name {
            if msg.nl_type == Rtm::Newlink {
                print!("NEWLINK");
            } else if msg.nl_type == Rtm::Dellink {
                print!("DELLINK");
            } else {
                print!("???LINK");
            }
            print!(" {}: {}", p.ifi_index, name);
            if flags.contains(&Iff::Up) {
                print!(" UP");
            }
            if flags.contains(&Iff::Running) {
                print!(" RUNNING");
            }
            if flags.contains(&Iff::Broadcast) {
                print!(" BROADCAST");
            }
            if flags.contains(&Iff::Multicast) {
                print!(" MULTICAST");
            }
            println!("");
        }
    }
}

async fn get_addrs(mut ss: NlSocket) -> Result<(), Box<dyn Error>> {
    loop {
        let mut buffer = Vec::new();
        let msgs = ss.recv(&mut buffer).await?;
        //println!("msgs: {:?}\n\n", msgs);
        for msg in msgs {
            if let NlPayload::Err(e) = msg.nl_payload {
                if e.error == -2 {
                    println!(
                        "This test is not supported on this machine as it requires nl80211; skipping"
                );
                } else {
                    println!("Error {:?}", e);
                }
            } else {
                handle(msg);
            }
        }
    }
}

async fn get_links(mut ss: NlSocket) -> Result<(), Box<dyn Error>> {
    loop {
        let mut buffer = Vec::new();
        let msgs = ss.recv(&mut buffer).await?;
        //println!("msgs: {:?}\n\n", msgs);
        for msg in msgs {
            if let NlPayload::Err(e) = msg.nl_payload {
                if e.error == -2 {
                    println!(
                        "This test is not supported on this machine as it requires nl80211; skipping"
                );
                } else {
                    println!("Error {:?}", e);
                }
            } else {
                handle_link(msg);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut sock = NlSocketHandle::connect(
        NlFamily::Route, /* family */
        None,
        &[5],               /* groups 1=LINK 5=ADDR */
    )?;

    let mut ss = NlSocket::new(sock)?;

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
    ss.send(&nl_header).await?;

    let f1 = get_addrs(ss);

    let mut sock2 = NlSocketHandle::connect(
        NlFamily::Route, /* family */
        None,
        &[1],               /* groups 1=LINK 5=ADDR */
    )?;

    let mut ss2 = NlSocket::new(sock2)?;

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
        Rtm::Getlink, //addr,
        NlmFFlags::new(&[NlmF::Request, NlmF::Match]),
        None,
        None,
        NlPayload::Payload(ifinfomsg),
    );

    ss2.send(&nl_header).await?;

    let f2 = get_links(ss2);

    try_join!(f1, f2);

    Ok(())
}
