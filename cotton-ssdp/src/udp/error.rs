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
    /// Something else not implemented
    NotImplemented,

    /// A system call returned an error
    #[cfg(feature = "std")]
    Syscall(Syscall, ::std::io::Error),

    /// A smoltcp multicast call returned an error
    #[cfg(feature = "smoltcp")]
    SmoltcpMulticast(Syscall, ::smoltcp::iface::MulticastError),

    /// A smoltcp send call returned an error
    #[cfg(feature = "smoltcp")]
    SmoltcpUdpSend(::smoltcp::socket::udp::SendError),
}

impl ::core::fmt::Display for Error {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match self {
            Self::NoPacketInfo => f.write_str("recvmsg: no pktinfo returned"),
            Self::Ipv6NotImplemented => f.write_str("IPv6 not implemented"),
            Self::NotImplemented => f.write_str("not implemented"),

            #[cfg(feature = "std")]
            Self::Syscall(s, _) => write!(f, "error from syscall {s:?}"),

            #[cfg(feature = "smoltcp")]
            Self::SmoltcpMulticast(s, e) => {
                write!(f, "error from smoltcp {s:?}: {e:?}")
            }

            #[cfg(feature = "smoltcp")]
            Self::SmoltcpUdpSend(e) => {
                write!(f, "error from smoltcp UDP send: {e:?}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl ::std::error::Error for Error {
    fn source(&self) -> Option<&(dyn ::std::error::Error + 'static)> {
        // NB smoltcp errors do not implement std::Error
        match self {
            Self::Syscall(_, e) => Some(e),
            _ => None,
        }
    }
}

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
    fn display_nyi_error() {
        use ::std::error::Error;

        let e = super::Error::NotImplemented;
        let m = format!("{e}");
        assert_eq!(m, "not implemented".to_string());

        assert!(e.source().is_none());
    }

    #[test]
    fn debug_nyi_error() {
        let e = super::Error::NotImplemented;
        let e = format!("{e:?}");
        assert_eq!(e, "NotImplemented".to_string());
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

    #[test]
    #[cfg(feature = "smoltcp")]
    fn display_smoltcp_error() {
        let e = Error::SmoltcpMulticast(
            Syscall::JoinMulticast,
            ::smoltcp::iface::MulticastError::Exhausted,
        );
        let m = format!("{e}");
        assert_eq!(
            m,
            "error from smoltcp JoinMulticast: Exhausted".to_string()
        );
    }

    #[test]
    #[cfg(feature = "smoltcp")]
    fn debug_smoltcp_error() {
        let e = Error::SmoltcpMulticast(
            Syscall::JoinMulticast,
            ::smoltcp::iface::MulticastError::Exhausted,
        );
        let e = format!("{e:?}");
        assert_eq!(
            e,
            "SmoltcpMulticast(JoinMulticast, Exhausted)".to_string()
        );
    }

    #[test]
    #[cfg(feature = "smoltcp")]
    fn display_smoltcp_udp_send_error() {
        let e = Error::SmoltcpUdpSend(
            ::smoltcp::socket::udp::SendError::BufferFull,
        );
        let m = format!("{e}");
        assert_eq!(m, "error from smoltcp UDP send: BufferFull".to_string());
    }

    #[test]
    #[cfg(feature = "smoltcp")]
    fn debug_smoltcp_udp_send_error() {
        let e = Error::SmoltcpUdpSend(
            ::smoltcp::socket::udp::SendError::BufferFull,
        );
        let e = format!("{e:?}");
        assert_eq!(e, "SmoltcpUdpSend(BufferFull)".to_string());
    }
}
