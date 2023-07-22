use cotton_netif::InterfaceIndex;
use no_std_net::{IpAddr, SocketAddr};

/// The list of system calls which can return errors
#[non_exhaustive]
#[derive(Debug)]
pub enum Syscall {
    /// recvmsg() returned an error
    Recvmsg,
    /// sendmsg() returned an error
    Sendmsg,
    /// setsockopt(IP_ADD_MEMBERSHIP) returned an error
    JoinMulticast,
    /// setsockopt(IP_DROP_MEMBERSHIP) returned an error
    LeaveMulticast,
}

/// The errors which can be returned from UDP trait methods
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// recvmsg didn't return packet info as expected
    NoPacketInfo,
    /// IPv6 attempted (NYI)
    Ipv6NotImplemented,

    /// A system call returned an error
    #[cfg(feature = "std")]
    Syscall(Syscall, ::std::io::Error),
}

impl ::core::fmt::Display for Error {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match self {
            Self::NoPacketInfo => f.write_str("recvmsg: no pktinfo returned"),
            Self::Ipv6NotImplemented => f.write_str("IPv6 not implemented"),

            #[cfg(feature = "std")]
            Self::Syscall(s, _) => write!(f, "error from syscall {s:?}"),
        }
    }
}

#[cfg(feature = "std")]
impl ::std::error::Error for Error {
    fn source(&self) -> Option<&(dyn ::std::error::Error + 'static)> {
        match self {
            Self::Syscall(_, e) => Some(e),
            _ => None,
        }
    }
}

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
    ) -> Result<(), Error>
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
    ) -> Result<(usize, IpAddr, SocketAddr), Error>;
}

/// Joining and leaving multicast groups (by interface number)
pub trait Multicast {
    /// Join a particular multicast group on a particular network interface
    ///
    /// # Errors
    ///
    /// Can only fail if the underlying system call fails.
    ///
    fn join_multicast_group(
        &self,
        multicast_address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), Error>;

    /// Leave a particular multicast group on a particular network interface
    ///
    /// # Errors
    ///
    /// Can only fail if the underlying system call fails.
    ///
    fn leave_multicast_group(
        &self,
        multicast_address: &IpAddr,
        interface: InterfaceIndex,
    ) -> Result<(), Error>;
}

/// Utilities common to all implementations using `std::net` underneath
#[cfg(any(feature = "sync", feature = "async"))]
pub mod std;

/// Trait implementations for MIO sockets
#[cfg(feature = "sync")]
pub mod mio;

/// Trait implementations for Tokio sockets
#[cfg(feature = "async")]
pub mod tokio;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    #[cfg(feature = "std")]
    fn display_pkt_error() {
        use ::std::error::Error;

        let e = super::Error::NoPacketInfo;
        let m = format!("{e}");
        assert_eq!(m, "recvmsg: no pktinfo returned".to_string());

        assert!(e.source().is_none());
    }

    #[test]
    fn debug_pkt_error() {
        let e = Error::NoPacketInfo;
        let e = format!("{e:?}");
        assert_eq!(e, "NoPacketInfo".to_string());
    }

    #[test]
    #[cfg(feature = "std")]
    fn display_ipv6_error() {
        use ::std::error::Error;

        let e = super::Error::Ipv6NotImplemented;
        let m = format!("{e}");
        assert_eq!(m, "IPv6 not implemented".to_string());

        assert!(e.source().is_none());
    }

    #[test]
    fn debug_ipv6_error() {
        let e = super::Error::Ipv6NotImplemented;
        let e = format!("{e:?}");
        assert_eq!(e, "Ipv6NotImplemented".to_string());
    }

    #[test]
    #[cfg(feature = "std")]
    fn display_syscall_error() {
        use ::std::error::Error;

        let e = super::Error::Syscall(
            Syscall::JoinMulticast,
            ::std::io::Error::new(::std::io::ErrorKind::Other, "injected"),
        );
        let m = format!("{e}");
        assert_eq!(m, "error from syscall JoinMulticast".to_string());

        let m = format!("{}", e.source().unwrap());
        assert_eq!(m, "injected".to_string());
    }

    #[test]
    #[cfg(feature = "std")]
    fn debug_syscall_error() {
        let e = Error::Syscall(
            Syscall::JoinMulticast,
            ::std::io::Error::new(::std::io::ErrorKind::Other, "injected"),
        );
        let e = format!("{e:?}");
        assert_eq!(e, "Syscall(JoinMulticast, Custom { kind: Other, error: \"injected\" })".to_string());
    }
}
