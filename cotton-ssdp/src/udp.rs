use core::net::{IpAddr, SocketAddr};
use cotton_netif::InterfaceIndex;

/// An error type for UDP system-call errors
pub mod error;

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
#[cfg(feature = "std")]
pub mod std;

/// Trait implementations for MIO sockets
#[cfg(feature = "sync")]
pub mod mio;

/// Trait implementations for Tokio sockets
#[cfg(feature = "async")]
pub mod tokio;

pub mod smoltcp;

/// Trait implementations for Smoltcp sockets
#[cfg(feature = "smoltcp")]
pub use error::{Error, Syscall};
