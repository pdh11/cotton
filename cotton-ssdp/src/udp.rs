use cotton_netif::InterfaceIndex;
use nix::cmsg_space;
use nix::sys::socket::ControlMessage;
use nix::sys::socket::ControlMessageOwned;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::SockaddrStorage;
use std::io::IoSlice;
use std::io::IoSliceMut;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;

/// Sending UDP datagrams from a specific source IP
pub trait TargetedSend {
    /// Send a UDP datagram from a specific source IP (and interface)
    ///
    /// Works even if two interfaces share the same IP range
    /// (169.254/16, for instance), so long as they have different
    /// addresses.
    ///
    /// For how this works see
    /// <https://man7.org/linux/man-pages/man7/ip.7.html>
    ///
    /// This facility probably only works on Linux.
    ///
    /// The interface is agnostic about IPv4/IPv6, but the current
    /// implementation is IPv4-only.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the underlying sendmsg call fails, or
    /// (currently) if IPv6 is attempted.
    ///
    fn send_with<F>(
        &self,
        size: usize,
        to: &SocketAddr,
        from: &IpAddr,
        f: F,
    ) -> Result<(), std::io::Error>
    where
        F: FnOnce(&mut [u8]) -> usize;
}

/// Receiving UDP datagrams, recording which IP we received it on
pub trait TargetedReceive {
    /// Receive a UDP datagram, recording which IP we received it on
    ///
    /// This is not the same as which IP it was addressed to (e.g. in
    /// the case of broadcast packets); it's the IP from which the
    /// peer would be expecting a reply to originate.
    ///
    /// The socket must have its `Ipv4PacketInfo` option enabled,
    /// using some equivalent of
    /// `nix::sys::socket::setsockopt`(`s.as_raw_fd`(),
    /// `nix::sys::socket::sockopt::Ipv4PacketInfo`, &true)?;
    ///
    /// The interface is agnostic about IPv4/IPv6, but the current
    /// implementation is IPv4-only.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the underlying recvmsg call fails, if no
    /// packet info is received (check the `setsockopt`), or
    /// (currently) if IPv6 is attempted.
    ///
    fn receive_to(
        &self,
        buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, SocketAddr), std::io::Error>;
}

pub trait Multicast {
    fn join_multicast_group(
        &self,
        multicast_address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error>;

    fn leave_multicast_group(
        &self,
        multicast_address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error>;
}

fn send_from(
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

fn receive_to(
    fd: RawFd,
    buffer: &mut [u8],
) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
    /* The inner function does most of the work, and is parameterised on
     * the recvmsg call purely for testing reasons.
     */
    receive_to_inner(fd, buffer, receive_using_recvmsg)
}

#[allow(clippy::cast_possible_truncation)] // socklen_t
#[allow(clippy::cast_possible_wrap)] // ifindex
fn ipv4_multicast_operation(
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
                imr_ifindex: interface.0 as libc::c_int,
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

/* ==== Implementations for tokio::net::UdpSocket ==== */

impl TargetedSend for tokio::net::UdpSocket {
    fn send_with<F>(
        &self,
        size: usize,
        to: &SocketAddr,
        from: &IpAddr,
        f: F,
    ) -> Result<(), std::io::Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let mut buffer = vec![0u8; size];
        let actual_size = f(&mut buffer);
        self.try_io(tokio::io::Interest::WRITABLE, || {
            send_from(self.as_raw_fd(), &buffer[0..actual_size], to, from)
        })
    }
}

impl TargetedReceive for tokio::net::UdpSocket {
    fn receive_to(
        &self,
        buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
        self.try_io(tokio::io::Interest::READABLE, || {
            receive_to(self.as_raw_fd(), buffer)
        })
    }
}

impl Multicast for tokio::net::UdpSocket {
    fn join_multicast_group(
        &self,
        address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error> {
        ipv4_multicast_operation(
            self.as_raw_fd(),
            libc::IP_ADD_MEMBERSHIP,
            address,
            interface,
        )
    }

    fn leave_multicast_group(
        &self,
        address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error> {
        ipv4_multicast_operation(
            self.as_raw_fd(),
            libc::IP_DROP_MEMBERSHIP,
            address,
            interface,
        )
    }
}

/* ==== Implementations for mio::net::UdpSocket ==== */

impl TargetedSend for mio::net::UdpSocket {
    fn send_with<F>(
        &self,
        size: usize,
        to: &SocketAddr,
        from: &IpAddr,
        f: F,
    ) -> Result<(), std::io::Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let mut buffer = vec![0u8; size];
        let actual_size = f(&mut buffer);
        self.try_io(|| {
            send_from(self.as_raw_fd(), &buffer[0..actual_size], to, from)
        })
    }
}

impl TargetedReceive for mio::net::UdpSocket {
    fn receive_to(
        &self,
        buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, SocketAddr), std::io::Error> {
        self.try_io(|| receive_to(self.as_raw_fd(), buffer))
    }
}

impl Multicast for mio::net::UdpSocket {
    fn join_multicast_group(
        &self,
        address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error> {
        ipv4_multicast_operation(
            self.as_raw_fd(),
            libc::IP_ADD_MEMBERSHIP,
            address,
            interface,
        )
    }

    fn leave_multicast_group(
        &self,
        address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), std::io::Error> {
        ipv4_multicast_operation(
            self.as_raw_fd(),
            libc::IP_DROP_MEMBERSHIP,
            address,
            interface,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::socket::setsockopt;
    use nix::sys::socket::sockopt::Ipv4PacketInfo;
    use std::net::Ipv6Addr;
    use std::net::SocketAddrV6;

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
    ) -> Result<(usize, IpAddr, Option<SockaddrStorage>), std::io::Error> {
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
    ) -> Result<(usize, IpAddr, Option<SockaddrStorage>), std::io::Error> {
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

    #[test]
    #[cfg_attr(miri, ignore)]
    fn tokio_traits() {
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        tx.set_nonblocking(true).unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();

        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let tx = tokio::net::UdpSocket::from_std(tx).unwrap();
                let rx = tokio::net::UdpSocket::from_std(rx).unwrap();

                tx.writable().await.unwrap();
                let r = tx.send_with(
                    512,
                    &SocketAddr::new(localhost, rx_port),
                    &IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    |b| {
                        b[0..3].copy_from_slice(b"foo");
                        3
                    },
                );
                assert!(r.is_ok());

                rx.readable().await.unwrap();

                let mut buf = [0u8; 1500];
                let r = rx.receive_to(&mut buf);
                let (n, wasto, wasfrom) = r.unwrap();
                assert!(n == 3);
                assert!(wasto == localhost);
                assert!(wasfrom == SocketAddr::new(localhost, tx_port));

                let r = rx.join_multicast_group(
                    &IpAddr::V4("127.0.0.1".parse().unwrap()),
                    InterfaceIndex(1),
                ); // Not a mcast addr
                assert!(r.is_err());

                let r = rx.join_multicast_group(
                    &IpAddr::V6("::1".parse().unwrap()),
                    InterfaceIndex(1),
                ); // IPv6 NYI
                assert!(r.is_err());

                let r = rx.join_multicast_group(
                    &IpAddr::V4("239.255.255.250".parse().unwrap()),
                    InterfaceIndex(1),
                );
                assert!(r.is_ok());

                let r = rx.leave_multicast_group(
                    &IpAddr::V6("::1".parse().unwrap()),
                    InterfaceIndex(1),
                ); // IPv6 NYI
                assert!(r.is_err());

                let r = rx.leave_multicast_group(
                    &IpAddr::V4("127.0.0.1".parse().unwrap()),
                    InterfaceIndex(1),
                ); // Not a mcast addr
                assert!(r.is_err());

                let r = rx.leave_multicast_group(
                    &IpAddr::V4("239.255.255.250".parse().unwrap()),
                    InterfaceIndex(1),
                );
                assert!(r.is_ok());
            });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn mio_traits() {
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        tx.set_nonblocking(true).unwrap();
        let tx_port = tx.local_addr().unwrap().port();
        let rx = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        rx.set_nonblocking(true).unwrap();
        setsockopt(rx.as_raw_fd(), Ipv4PacketInfo, &true).unwrap();
        let rx_port = rx.local_addr().unwrap().port();

        let tx = mio::net::UdpSocket::from_std(tx);
        let rx = mio::net::UdpSocket::from_std(rx);

        let r = tx.send_with(
            512,
            &SocketAddr::new(localhost, rx_port),
            &IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            |b| {
                b[0..3].copy_from_slice(b"foo");
                3
            },
        );
        assert!(r.is_ok());

        let mut buf = [0u8; 1500];
        let r = rx.receive_to(&mut buf);
        let (n, wasto, wasfrom) = r.unwrap();
        assert!(n == 3);
        assert!(wasto == localhost);
        assert!(wasfrom == SocketAddr::new(localhost, tx_port));

        let r = rx.join_multicast_group(
            &IpAddr::V4("127.0.0.1".parse().unwrap()),
            InterfaceIndex(1),
        ); // Not a mcast addr
        assert!(r.is_err());

        let r = rx.join_multicast_group(
            &IpAddr::V6("::1".parse().unwrap()),
            InterfaceIndex(1),
        ); // IPv6 NYI
        assert!(r.is_err());

        let r = rx.join_multicast_group(
            &IpAddr::V4("239.255.255.250".parse().unwrap()),
            InterfaceIndex(1),
        );
        assert!(r.is_ok());

        let r = rx.leave_multicast_group(
            &IpAddr::V6("::1".parse().unwrap()),
            InterfaceIndex(1),
        ); // IPv6 NYI
        assert!(r.is_err());

        let r = rx.leave_multicast_group(
            &IpAddr::V4("127.0.0.1".parse().unwrap()),
            InterfaceIndex(1),
        ); // Not a mcast addr
        assert!(r.is_err());

        let r = rx.leave_multicast_group(
            &IpAddr::V4("239.255.255.250".parse().unwrap()),
            InterfaceIndex(1),
        );
        assert!(r.is_ok());
    }
}
