use cotton_netif::InterfaceIndex;
use nix::cmsg_space;
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::Ipv4PacketInfo;
use nix::sys::socket::ControlMessage;
use nix::sys::socket::ControlMessageOwned;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::SockaddrStorage;
use std::io::IoSlice;
use std::io::IoSliceMut;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;

type NewSocketFn = fn() -> std::io::Result<socket2::Socket>;
type SockoptFn = fn(&socket2::Socket, bool) -> std::io::Result<()>;
type RawSockoptFn =
    fn(&socket2::Socket, bool) -> Result<(), nix::errno::Errno>;
type BindFn =
    fn(&socket2::Socket, std::net::SocketAddrV4) -> std::io::Result<()>;

fn setup_socket_inner(
    port: u16,
    new_socket: NewSocketFn,
    nonblocking: SockoptFn,
    reuse_address: SockoptFn,
    bind: BindFn,
    ipv4_packetinfo: RawSockoptFn,
) -> std::io::Result<std::net::UdpSocket> {
    let socket = new_socket()?;
    nonblocking(&socket, true)?;
    reuse_address(&socket, true)?;
    bind(
        &socket,
        std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, port),
    )?;
    ipv4_packetinfo(&socket, true)?;
    Ok(socket.into())
}

pub(crate) fn setup_socket(
    port: u16,
) -> Result<std::net::UdpSocket, std::io::Error> {
    setup_socket_inner(
        port,
        || {
            socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::DGRAM,
                None,
            )
        },
        socket2::Socket::set_nonblocking,
        socket2::Socket::set_reuse_address,
        |s, a| s.bind(&socket2::SockAddr::from(a)),
        |s, b| setsockopt(s.as_raw_fd(), Ipv4PacketInfo, &b),
    )
}

#[allow(clippy::cast_possible_truncation)] // socklen_t
#[allow(clippy::cast_possible_wrap)] // ifindex
pub(crate) fn ipv4_multicast_operation(
    fd: RawFd,
    op: libc::c_int,
    multicast_address: &IpAddr,
    interface: InterfaceIndex,
) -> Result<(), std::io::Error> {
    match *multicast_address {
        IpAddr::V4(mcast) => {
            // The tokio socket API (and indeed the std::net one) only
            // allow joining by IP address, for IPv4 at least. But that's
            // not robust, and Linux at least has long supported joining
            // by interface index. We need to use a lower-level API to
            // access that.
            let mreqn = libc::ip_mreqn {
                imr_multiaddr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(mcast.octets()),
                },
                imr_address: libc::in_addr { s_addr: 0 },
                imr_ifindex: interface.0.get() as libc::c_int,
            };
            unsafe {
                let ret = libc::setsockopt(
                    fd,
                    libc::IPPROTO_IP,
                    op,
                    std::ptr::addr_of!(mreqn).cast::<libc::c_void>(),
                    std::mem::size_of_val(&mreqn) as libc::socklen_t,
                );
                if ret != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        }
        IpAddr::V6(_) => Err(std::io::ErrorKind::Unsupported.into()),
    }
}

pub(crate) fn send_from(
    fd: RawFd,
    buffer: &[u8],
    to: &SocketAddr,
    from: &IpAddr,
) -> Result<(), std::io::Error> {
    if let IpAddr::V4(from) = from {
        let iov = [IoSlice::new(buffer)];
        let pi = libc::in_pktinfo {
            ipi_ifindex: 0,
            ipi_addr: libc::in_addr { s_addr: 0 },
            ipi_spec_dst: libc::in_addr {
                s_addr: u32::to_be((*from).into()),
            },
        };

        let cmsg = ControlMessage::Ipv4PacketInfo(&pi);
        let dest = match to {
            SocketAddr::V4(ipv4) => SockaddrStorage::from(*ipv4),
            SocketAddr::V6(ipv6) => SockaddrStorage::from(*ipv6),
        };
        let r = nix::sys::socket::sendmsg(
            fd,
            &iov,
            &[cmsg],
            MsgFlags::empty(),
            Some(&dest),
        );
        if let Err(e) = r {
            println!("sendmsg {e:?}");
            return Err(e.into());
        }
        // println!("sendmsg to {:?} OK", to);
        Ok(())
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "IPv6 NYI"))
    }
}

fn receive_using_recvmsg(
    fd: RawFd,
    buffer: &mut [u8],
) -> Result<(usize, IpAddr, Option<SockaddrStorage>), std::io::Error> {
    let mut cmsgspace = cmsg_space!(libc::in_pktinfo);
    let mut iov = [IoSliceMut::new(buffer)];
    let r = nix::sys::socket::recvmsg::<SockaddrStorage>(
        fd,
        &mut iov,
        Some(&mut cmsgspace),
        MsgFlags::empty(),
    )?;
    let Some(ControlMessageOwned::Ipv4PacketInfo(pi)) = r.cmsgs().next() else {
        println!("receive: no pktinfo");
        return Err(std::io::ErrorKind::InvalidData.into());
     };
    let rxon = Ipv4Addr::from(u32::from_be(pi.ipi_spec_dst.s_addr));
    Ok((r.bytes, IpAddr::V4(rxon), r.address))
}

/** The type of `receive_using_recvmsg`
 */
type ReceiveInnerFn =
    fn(
        RawFd,
        &mut [u8],
    )
        -> Result<(usize, IpAddr, Option<SockaddrStorage>), std::io::Error>;

fn receive_to_inner(
    fd: RawFd,
    buffer: &mut [u8],
    recvmsg: ReceiveInnerFn,
) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
    let (bytes, rxon, address) = recvmsg(fd, buffer)?;

    //println!("recvmsg ok");
    let wasfrom = {
        if let Some(ss) = address {
            if let Some(sin) = ss.as_sockaddr_in() {
                SocketAddrV4::new(Ipv4Addr::from(sin.ip()), sin.port())
            } else {
                //println!("receive: wasfrom not ipv4 {:?}", ss);
                return Err(std::io::ErrorKind::InvalidData.into());
            }
        } else {
            //println!("receive: wasfrom no address");
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    };
    Ok((bytes, rxon, SocketAddr::V4(wasfrom)))
}

pub(crate) fn receive_to(
    fd: RawFd,
    buffer: &mut [u8],
) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
    /* The inner function does most of the work, and is parameterised on
     * the recvmsg call purely for testing reasons.
     */
    receive_to_inner(fd, buffer, receive_using_recvmsg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::socket::setsockopt;
    use nix::sys::socket::sockopt::Ipv4PacketInfo;
    use std::net::Ipv6Addr;
    use std::net::SocketAddrV6;

    fn my_err() -> ::std::io::Error {
        ::std::io::Error::from(::std::io::ErrorKind::Other)
    }

    fn bogus_new_socket() -> ::std::io::Result<socket2::Socket> {
        Err(my_err())
    }
    fn bogus_setsockopt(
        _: &socket2::Socket,
        b: bool,
    ) -> ::std::io::Result<()> {
        assert!(b);
        Err(my_err())
    }
    fn bogus_raw_setsockopt(
        _: &socket2::Socket,
        _: bool,
    ) -> Result<(), nix::errno::Errno> {
        Err(nix::errno::Errno::ENOTTY)
    }
    fn bogus_bind(
        _: &socket2::Socket,
        _: std::net::SocketAddrV4,
    ) -> ::std::io::Result<()> {
        Err(my_err())
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn setup_socket_passes_on_creation_error() {
        let e = setup_socket_inner(
            0u16,
            bogus_new_socket,
            bogus_setsockopt,
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn setup_socket_passes_on_nonblocking_error() {
        let e = setup_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            bogus_setsockopt,
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn setup_socket_passes_on_reuseaddr_error() {
        let e = setup_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            socket2::Socket::set_nonblocking,
            bogus_setsockopt,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn setup_socket_passes_on_bind_error() {
        let e = setup_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            socket2::Socket::set_nonblocking,
            socket2::Socket::set_reuse_address,
            bogus_bind,
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn setup_socket_passes_on_pktinfo_error() {
        let e = setup_socket_inner(
            0u16,
            || {
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    None,
                )
            },
            socket2::Socket::set_nonblocking,
            socket2::Socket::set_reuse_address,
            |s, a| s.bind(&socket2::SockAddr::from(a)),
            bogus_raw_setsockopt,
        );

        assert!(e.is_err());
    }

    fn local_ipv4() -> Option<Ipv4Addr> {
        cotton_netif::get_interfaces().unwrap().find_map(|e| {
            if let cotton_netif::NetworkEvent::NewAddr(_, IpAddr::V4(a), _) = e
            {
                if a == Ipv4Addr::LOCALHOST {
                    None
                } else {
                    Some(a)
                }
            } else {
                None
            }
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn localhost_source_localhost_dest() {
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(localhost, rx_port),
            &IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        )
        .is_ok());
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        let (n, wasto, wasfrom) = r.unwrap();
        assert!(n == 3);
        assert!(wasto == localhost);
        assert!(wasfrom == SocketAddr::new(localhost, tx_port));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn localhost_source_real_dest() {
        let ipv4 = IpAddr::V4(local_ipv4().unwrap());
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(ipv4, rx_port),
            &IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        )
        .is_ok());
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        let (n, wasto, wasfrom) = r.unwrap();
        assert!(n == 3);
        assert!(wasto == ipv4);
        assert!(wasfrom == SocketAddr::new(localhost, tx_port));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn real_source_localhost_dest() {
        let ipv4 = IpAddr::V4(local_ipv4().unwrap());
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(localhost, rx_port),
            &ipv4,
        )
        .is_ok());
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        let (n, wasto, wasfrom) = r.unwrap();
        assert!(n == 3);
        assert!(wasto == localhost);
        assert!(wasfrom == SocketAddr::new(ipv4, tx_port));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn real_source_real_dest() {
        let ipv4 = IpAddr::V4(local_ipv4().unwrap());
        //let localhost = IpAddr::V4(Ipv4Addr::new(127,0,0,1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(ipv4, rx_port),
            &ipv4,
        )
        .is_ok());
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        let (n, wasto, wasfrom) = r.unwrap();
        assert!(n == 3);
        assert!(wasto == ipv4);
        assert!(wasfrom == SocketAddr::new(ipv4, tx_port));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn ipv6_source_fails() {
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(localhost, 0),
            &IpAddr::V6(Ipv6Addr::LOCALHOST)
        )
        .is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn ipv6_dest_fails() {
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0),
            &localhost
        )
        .is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn recvmsg_error_passed_on() {
        let mut buf = [0u8; 1500];
        assert!(receive_to(0 as RawFd, &mut buf).is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn recvmsg_no_cmsg_is_error() {
        // cf. localhost_source_localhost_dest()
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        //let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        // But! we forget to do the setsockopt:
        //setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        assert!(send_from(
            tx.as_raw_fd(),
            b"foo",
            &SocketAddr::new(localhost, rx_port),
            &IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        )
        .is_ok());
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        assert!(r.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn recvmsg_ipv6_is_error() {
        let localhost = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let tx = std::net::UdpSocket::bind("::1:0").unwrap();
        //let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("::0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        //setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();
        tx.send_to(b"foo", SocketAddr::new(localhost, rx_port))
            .unwrap();
        let mut buf = [0u8; 1500];
        let r = receive_to(rx.as_raw_fd(), &mut buf);
        assert!(r.is_err());
    }

    #[allow(clippy::unnecessary_wraps)] // needs to match API
    fn mock_recvmsg_no_address(
        _fd: RawFd,
        _buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, Option<SockaddrStorage>), ::std::io::Error>
    {
        Ok((3, IpAddr::V4(Ipv4Addr::LOCALHOST), None))
    }

    #[test]
    fn recvmsg_no_address_is_error() {
        /* nix::sys::socket::recvmsg always returns a Some(address), making
         * it hard to get coverage of the None case. So we cover that case
         * using a replacement for recvmsg.
         */
        let mut buf = [0u8; 1500];
        assert!(receive_to_inner(
            0 as RawFd,
            &mut buf,
            mock_recvmsg_no_address
        )
        .is_err());
    }

    #[allow(clippy::unnecessary_wraps)] // needs to match API
    fn mock_recvmsg_not_ipv4(
        _fd: RawFd,
        _buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, Option<SockaddrStorage>), ::std::io::Error>
    {
        Ok((
            3,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            Some(SockaddrStorage::from(SocketAddrV6::new(
                Ipv6Addr::LOCALHOST,
                80,
                0,
                0,
            ))),
        ))
    }

    #[test]
    fn recvmsg_not_ipv4_is_error() {
        let mut buf = [0u8; 1500];
        assert!(
            receive_to_inner(0 as RawFd, &mut buf, mock_recvmsg_not_ipv4)
                .is_err()
        );
    }
}
