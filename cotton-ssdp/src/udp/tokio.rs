use super::{Error, Syscall};
use std::net::{IpAddr, SocketAddr};

impl super::TargetedSend for tokio::net::UdpSocket {
    fn send_with<F>(
        &self,
        size: usize,
        to: &SocketAddr,
        from: &IpAddr,
        f: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let mut buffer = vec![0u8; size];
        let actual_size = f(&mut buffer);
        self.try_io(tokio::io::Interest::WRITABLE, || {
            super::std::send_from(self, &buffer[0..actual_size], to, from)
        })
        .map_err(|e| Error::Syscall(Syscall::Sendmsg, e))
    }
}

impl super::TargetedReceive for tokio::net::UdpSocket {
    fn receive_to(
        &self,
        buffer: &mut [u8],
    ) -> Result<(usize, IpAddr, SocketAddr), Error> {
        self.try_io(tokio::io::Interest::READABLE, || {
            super::std::receive_to(self, buffer)
        })
        .map_err(|e| Error::Syscall(Syscall::Recvmsg, e))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Multicast, TargetedReceive, TargetedSend};
    use super::*;
    use cotton_netif::InterfaceIndex;
    use nix::sys::socket::setsockopt;
    use nix::sys::socket::sockopt::Ipv4PacketInfo;
    use std::net::Ipv4Addr;

    fn make_index(i: u32) -> InterfaceIndex {
        InterfaceIndex(core::num::NonZeroU32::new(i).unwrap())
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
        setsockopt(&rx, Ipv4PacketInfo, &true).unwrap();
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
                    make_index(1),
                ); // Not a mcast addr
                assert!(r.is_err());

                let r = rx.join_multicast_group(
                    &IpAddr::V6("::1".parse().unwrap()),
                    make_index(1),
                ); // IPv6 NYI
                assert!(r.is_err());

                // "PowerPC" here really means "using QEMU", where
                // IP_{ADD/DEL}_MEMBERSHIP fail mysteriously
                // https://gitlab.com/qemu-project/qemu/-/issues/2553
                #[cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))]
                rx.join_multicast_group(
                    &IpAddr::V4("239.255.255.250".parse().unwrap()),
                    make_index(1),
                ).unwrap();

                let r = rx.leave_multicast_group(
                    &IpAddr::V6("::1".parse().unwrap()),
                    make_index(1),
                ); // IPv6 NYI
                assert!(r.is_err());

                let r = rx.leave_multicast_group(
                    &IpAddr::V4("127.0.0.1".parse().unwrap()),
                    make_index(1),
                ); // Not a mcast addr
                assert!(r.is_err());

                // "PowerPC" here really means "using QEMU", where
                // IP_{ADD/DEL}_MEMBERSHIP fail mysteriously
                // https://gitlab.com/qemu-project/qemu/-/issues/2553
                #[cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))]
                rx.leave_multicast_group(
                    &IpAddr::V4("239.255.255.250".parse().unwrap()),
                    make_index(1),
                ).unwrap();
            });
    }
}
