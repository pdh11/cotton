use crate::message;
use crate::message::Message;
use crate::udp;
use crate::{Advertisement, Notification};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use cotton_netif::{InterfaceIndex, NetworkEvent};
use no_std_net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use slotmap::SlotMap;

const MAX_PACKET_SIZE: usize = 512;

struct Interface {
    ips: Vec<IpAddr>,
    up: bool,
}

fn target_match(search: &str, candidate: &str) -> bool {
    if search == "ssdp:all" {
        return true;
    }
    if search == candidate {
        return true;
    }
    // UPnP DA 1.0 s1.2.3
    if let Some((sbase, sversion)) = search.rsplit_once(':') {
        if let Some((cbase, cversion)) = candidate.rsplit_once(':') {
            if sbase == cbase {
                if let Ok(sversion) = sversion.parse::<usize>() {
                    if let Ok(cversion) = cversion.parse::<usize>() {
                        return cversion >= sversion;
                    }
                }
            }
        }
    }
    false
}

fn rewrite_host(url: &str, ip: &IpAddr) -> String {
    let Some(prefix) = url.find("://") else { return url.to_string(); };

    if let Some(slash) = url[prefix + 3..].find('/') {
        if let Some(colon) = url[prefix + 3..].find(':') {
            if colon < slash {
                return url[..prefix + 3].to_string()
                    + &ip.to_string()
                    + &url[colon + prefix + 3..];
            }
        }
        return url[..prefix + 3].to_string()
            + &ip.to_string()
            + &url[slash + prefix + 3..];
    }
    url[..prefix + 3].to_string() + &ip.to_string()
}

/// A callback made by [`Engine`] when notification messages arrive
///
/// See implementations in [`crate::Service`] and [`crate::AsyncService`].
///
pub trait Callback {
    /// An SSDP notification has been received
    fn on_notification(&self, notification: &Notification);
}

struct ActiveSearch<CB: Callback> {
    notification_type: String,
    callback: CB,
}

slotmap::new_key_type! { struct ActiveSearchKey; }

/// The core of an SSDP implementation
///
/// This low-level facility is usually wrapped-up in
/// [`crate::Service`] or [`crate::AsyncService`] for use in larger
/// programs, but can also be used directly when needed (e.g. on
/// embedded systems).
///
/// This struct handles parsing and emitting SSDP messages; it does
/// not own or define the UDP sockets themselves, which are left to
/// its owner.  The owner should pass incoming UDP packets to
/// [`Engine::on_data`], and changes to available network interfaces
/// (if required) to [`Engine::on_network_event`].
///
/// The notifications should be retransmitted on a timer; the owner
/// should implement a timer facility, perhaps by using
/// [`crate::refresh_timer::RefreshTimer`], and call [`Engine::refresh`] each time the timer
/// expires. See, for instance, the `tokio::select!` loop in
/// `AsyncService::new_inner`.
///
pub struct Engine<CB: Callback> {
    interfaces: BTreeMap<InterfaceIndex, Interface>,
    active_searches: SlotMap<ActiveSearchKey, ActiveSearch<CB>>,
    advertisements: BTreeMap<String, Advertisement>,
}

impl<CB: Callback> Default for Engine<CB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<CB: Callback> Engine<CB> {
    /// Create a new Engine, parameterised by callback type
    ///
    #[must_use]
    pub fn new() -> Self {
        Engine {
            interfaces: BTreeMap::default(),
            active_searches: SlotMap::with_key(),
            advertisements: BTreeMap::default(),
        }
    }

    /// Re-send all messages
    ///
    /// This should be called periodically, perhaps with the help of a
    /// [`crate::refresh_timer::RefreshTimer`]
    pub fn refresh<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        socket: &SCK,
    ) {
        for (key, value) in &self.advertisements {
            self.notify_on_all(key, value, socket);
        }

        // If anybody is doing an ssdp:all search, then we don't need to
        // do any of the other searches.
        if let Some(all) = self
            .active_searches
            .iter()
            .find(|x| x.1.notification_type == "ssdp:all")
        {
            self.search_on_all(&all.1.notification_type, socket);
        } else {
            for s in self.active_searches.values() {
                self.search_on_all(&s.notification_type, socket);
            }
        }
    }

    fn search_on<SCK: udp::TargetedSend + udp::Multicast>(
        search_type: &str,
        source: &IpAddr,
        socket: &SCK,
    ) {
        let _ = socket.send_with(
            MAX_PACKET_SIZE,
            &"239.255.255.250:1900".parse().unwrap(),
            source,
            |b| message::build_search(b, search_type),
        );
    }

    fn search_on_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        search_type: &str,
        socket: &SCK,
    ) {
        for interface in self.interfaces.values() {
            if interface.up {
                for ip in &interface.ips {
                    Self::search_on(search_type, ip, socket);
                }
            }
        }
    }

    /// Subscribe to notifications of a particular service type
    ///
    /// And send searches.
    pub fn subscribe<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        notification_type: String,
        callback: CB,
        socket: &SCK,
    ) {
        self.search_on_all(&notification_type, socket);
        let s = ActiveSearch {
            notification_type,
            callback,
        };
        self.active_searches.insert(s);
    }

    fn call_subscribers(&self, notification: &Notification) {
        for s in self.active_searches.values() {
            match notification {
                Notification::ByeBye {
                    notification_type, ..
                }
                | Notification::Alive {
                    notification_type, ..
                } => {
                    if target_match(&s.notification_type, notification_type) {
                        s.callback.on_notification(notification);
                    }
                }
            }
        }
    }

    /// Notify the `Engine` that data is ready on one of its sockets
    pub fn on_data<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        buf: &[u8],
        socket: &SCK,
        wasto: IpAddr,
        wasfrom: SocketAddr,
    ) {
        if let Ok(m) = message::parse(buf) {
            match m {
                Message::NotifyAlive {
                    notification_type,
                    unique_service_name,
                    location,
                } => {
                    self.call_subscribers(&Notification::Alive {
                        notification_type,
                        unique_service_name,
                        location,
                    });
                }
                Message::NotifyByeBye {
                    notification_type,
                    unique_service_name,
                } => {
                    self.call_subscribers(&Notification::ByeBye {
                        notification_type,
                        unique_service_name,
                    });
                }
                Message::Search { search_target, .. } => {
                    for (key, value) in &self.advertisements {
                        if target_match(
                            &search_target,
                            &value.notification_type,
                        ) {
                            let url = rewrite_host(&value.location, &wasto);

                            let response_type = if search_target == "ssdp:all"
                            {
                                &value.notification_type
                            } else {
                                &search_target
                            };
                            let _ = socket.send_with(
                                MAX_PACKET_SIZE,
                                &wasfrom,
                                &wasto,
                                |b| {
                                    message::build_response(
                                        b,
                                        response_type,
                                        key,
                                        &url,
                                    )
                                },
                            );
                        }
                    }
                }
                Message::Response {
                    search_target,
                    unique_service_name,
                    location,
                } => {
                    self.call_subscribers(&Notification::Alive {
                        notification_type: search_target,
                        unique_service_name,
                        location,
                    });
                }
            };
        }
    }

    fn join_multicast<SCK: udp::TargetedSend + udp::Multicast>(
        interface: InterfaceIndex,
        multicast: &SCK,
    ) -> Result<(), udp::Error> {
        multicast.join_multicast_group(
            &IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)),
            interface,
        )
    }

    fn leave_multicast<SCK: udp::TargetedSend + udp::Multicast>(
        interface: InterfaceIndex,
        multicast: &SCK,
    ) -> Result<(), udp::Error> {
        multicast.leave_multicast_group(
            &IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)),
            interface,
        )
    }

    fn send_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        ips: &[IpAddr],
        search: &SCK,
    ) {
        for ip in ips {
            if let Some(all) = self
                .active_searches
                .iter()
                .find(|x| x.1.notification_type == "ssdp:all")
            {
                Self::search_on(&all.1.notification_type, ip, search);
            } else {
                for s in self.active_searches.values() {
                    Self::search_on(&s.notification_type, ip, search);
                }
            }

            for (key, value) in &self.advertisements {
                Self::notify_on(key, value, ip, search);
            }
        }
    }

    /// Notify the `Engine` of a network interface change
    ///
    /// # Errors
    ///
    /// Passes on errors from the underlying system-calls for joining
    /// (and leaving) multicast groups.
    pub fn on_network_event<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        e: &NetworkEvent,
        multicast: &SCK,
        search: &SCK,
    ) -> Result<(), udp::Error> {
        match e {
            NetworkEvent::NewLink(ix, _name, flags) => {
                if flags.contains(cotton_netif::Flags::MULTICAST) {
                    let up = flags.contains(
                        cotton_netif::Flags::RUNNING | cotton_netif::Flags::UP,
                    );
                    let mut do_send = false;
                    if let Some(v) = self.interfaces.get_mut(ix) {
                        if up && !v.up {
                            do_send = true;
                        }
                        v.up = up;
                    } else {
                        Self::join_multicast(*ix, multicast)?;
                        self.interfaces.insert(
                            *ix,
                            Interface {
                                ips: Vec::new(),
                                up,
                            },
                        );
                    }
                    if do_send {
                        self.send_all(&self.interfaces[ix].ips, search);
                    }
                }
            }
            NetworkEvent::DelLink(ix) => {
                if self.interfaces.remove(ix).is_some() {
                    Self::leave_multicast(*ix, multicast)?;
                }
            }
            NetworkEvent::NewAddr(ix, addr, _prefix) => {
                if addr.is_ipv4() {
                    // cotton-netif guarantees we get a NewLink before
                    // any NewAddr
                    if let Some(ref mut v) = self.interfaces.get_mut(ix) {
                        if !v.ips.contains(addr) {
                            v.ips.push(*addr);
                            if v.up {
                                self.send_all(&[*addr], search);
                            }
                        }
                    }
                }
            }
            NetworkEvent::DelAddr(ix, addr, _prefix) => {
                if let Some(ref mut v) = self.interfaces.get_mut(ix) {
                    if let Some(n) = v.ips.iter().position(|a| a == addr) {
                        v.ips.swap_remove(n);
                    }
                }
            }
        }
        Ok(())
    }

    fn notify_on<SCK: udp::TargetedSend + udp::Multicast>(
        unique_service_name: &str,
        advertisement: &Advertisement,
        source: &IpAddr,
        socket: &SCK,
    ) {
        let url = rewrite_host(&advertisement.location, source);
        let _ = socket.send_with(
            MAX_PACKET_SIZE,
            &SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(239, 255, 255, 250),
                1900,
            )),
            source,
            |b| {
                message::build_notify(
                    b,
                    &advertisement.notification_type,
                    unique_service_name,
                    &url,
                )
            },
        );
    }

    fn notify_on_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        unique_service_name: &str,
        advertisement: &Advertisement,
        socket: &SCK,
    ) {
        for interface in self.interfaces.values() {
            if interface.up {
                for ip in &interface.ips {
                    Self::notify_on(
                        unique_service_name,
                        advertisement,
                        ip,
                        socket,
                    );
                }
            }
        }
    }

    fn byebye_on<SCK: udp::TargetedSend + udp::Multicast>(
        unique_service_name: &str,
        notification_type: &str,
        source: &IpAddr,
        socket: &SCK,
    ) {
        let _ = socket.send_with(
            MAX_PACKET_SIZE,
            &SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(239, 255, 255, 250),
                1900,
            )),
            source,
            |b| {
                message::build_byebye(
                    b,
                    unique_service_name,
                    notification_type,
                )
            },
        );
    }

    fn byebye_on_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        notification_type: &str,
        unique_service_name: &str,
        socket: &SCK,
    ) {
        for interface in self.interfaces.values() {
            if interface.up {
                for ip in &interface.ips {
                    Self::byebye_on(
                        notification_type,
                        unique_service_name,
                        ip,
                        socket,
                    );
                }
            }
        }
    }

    /// Advertise a local resource to SSDP peers
    pub fn advertise<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        unique_service_name: String,
        advertisement: Advertisement,
        socket: &SCK,
    ) {
        self.notify_on_all(&unique_service_name, &advertisement, socket);
        self.advertisements
            .insert(unique_service_name, advertisement);
    }

    /// Withdraw an advertisement for a local resource
    ///
    /// For instance, it is "polite" to call this if shutting down
    /// cleanly.
    ///
    pub fn deadvertise<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        unique_service_name: &str,
        socket: &SCK,
    ) {
        if let Some(advertisement) =
            self.advertisements.remove(unique_service_name)
        {
            self.byebye_on_all(
                &advertisement.notification_type,
                unique_service_name,
                socket,
            );
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::message::parse;
    use no_std_net::{Ipv6Addr, SocketAddrV4};
    use std::sync::{Arc, Mutex};

    /* ==== Tests for target_match() ==== */

    #[test]
    fn target_match_ssdp_all() {
        assert!(target_match("ssdp:all", "upnp::rootdevice"));
        assert!(!target_match("upnp::rootdevice", "ssdp:all"));
    }

    #[test]
    fn target_match_equality() {
        assert!(target_match("upnp::rootdevice", "upnp::rootdevice"));
    }

    #[test]
    fn target_match_downlevel() {
        // If we search for CD:1 we should pick up CD:2's, but not vice versa
        assert!(target_match(
            "upnp::ContentDirectory:1",
            "upnp::ContentDirectory:2"
        ));
        assert!(!target_match(
            "upnp::ContentDirectory:2",
            "upnp::ContentDirectory:1"
        ));

        // Various noncanonical forms
        assert!(!target_match(
            "upnp::ContentDirectory",
            "upnp::ContentDirectory:1"
        ));
        assert!(!target_match(
            "upnp::ContentDirectory:1",
            "upnp::ContentDirectory"
        ));
        assert!(!target_match("fnord", "upnp::ContentDirectory:1"));
        assert!(!target_match("upnp::ContentDirectory:1", "fnord"));
        assert!(!target_match(
            "upnp::ContentDirectory:1",
            "upnp::ContentDirectory:X"
        ));
        assert!(!target_match(
            "upnp::ContentDirectory:X",
            "upnp::ContentDirectory:1"
        ));
    }

    #[derive(Default)]
    struct FakeSocket {
        sends: Mutex<Vec<(SocketAddr, IpAddr, Message)>>,
        mcasts: Mutex<Vec<(IpAddr, InterfaceIndex, bool)>>,
        injecting_multicast_error: bool,
    }

    impl FakeSocket {
        fn contains_send<F>(
            &self,
            wasto: SocketAddr,
            wasfrom: IpAddr,
            mut f: F,
        ) -> bool
        where
            F: FnMut(&Message) -> bool,
        {
            self.sends.lock().unwrap().iter().any(|(to, from, msg)| {
                *to == wasto && *from == wasfrom && f(msg)
            })
        }

        fn contains_search(&self, search: &str) -> bool {
            self.contains_send(multicast_dest(), LOCAL_SRC, |m| {
                matches!(m,
                             Message::Search { search_target, .. }
                             if search_target == search)
            })
        }

        fn no_sends(&self) -> bool {
            self.sends.lock().unwrap().is_empty()
        }

        fn send_count(&self) -> usize {
            self.sends.lock().unwrap().len()
        }

        fn contains_mcast(
            &self,
            group: IpAddr,
            interface: InterfaceIndex,
            join: bool,
        ) -> bool {
            self.mcasts.lock().unwrap().iter().any(|(gp, ix, jn)| {
                *gp == group && *ix == interface && *jn == join
            })
        }

        fn no_mcasts(&self) -> bool {
            self.mcasts.lock().unwrap().is_empty()
        }

        fn mcast_count(&self) -> usize {
            self.mcasts.lock().unwrap().len()
        }

        fn clear(&self) {
            self.sends.lock().unwrap().clear();
            self.mcasts.lock().unwrap().clear();
        }

        fn build_notify(notification_type: &str) -> Vec<u8> {
            let mut buf = [0u8; 512];

            let n = message::build_notify(
                &mut buf,
                notification_type,
                "uuid:37",
                "http://me",
            );
            buf[0..n].to_vec()
        }

        fn build_byebye(notification_type: &str) -> Vec<u8> {
            let mut buf = [0u8; 512];

            let n =
                message::build_byebye(&mut buf, notification_type, "uuid:37");
            buf[0..n].to_vec()
        }

        fn build_response(notification_type: &str) -> Vec<u8> {
            let mut buf = [0u8; 512];

            let n = message::build_response(
                &mut buf,
                notification_type,
                "uuid:37",
                "http://me",
            );
            buf[0..n].to_vec()
        }

        fn build_search(notification_type: &str) -> Vec<u8> {
            let mut buf = [0u8; 512];
            let n = message::build_search(&mut buf, notification_type);
            buf[0..n].to_vec()
        }

        fn inject_multicast_error(&mut self, errors: bool) {
            self.injecting_multicast_error = errors;
        }
    }

    impl udp::TargetedSend for FakeSocket {
        fn send_with<F>(
            &self,
            size: usize,
            to: &SocketAddr,
            from: &IpAddr,
            f: F,
        ) -> Result<(), udp::Error>
        where
            F: FnOnce(&mut [u8]) -> usize,
        {
            let mut buffer = vec![0u8; size];
            let actual_size = f(&mut buffer);
            self.sends.lock().unwrap().push((
                *to,
                *from,
                parse(&buffer[0..actual_size]).unwrap(),
            ));
            Ok(())
        }
    }

    impl udp::Multicast for FakeSocket {
        fn join_multicast_group(
            &self,
            multicast_address: &IpAddr,
            interface: InterfaceIndex,
        ) -> Result<(), udp::Error> {
            if self.injecting_multicast_error {
                Err(udp::Error::Syscall(
                    udp::Syscall::JoinMulticast,
                    std::io::Error::new(std::io::ErrorKind::Other, "injected"),
                ))
            } else {
                self.mcasts.lock().unwrap().push((
                    *multicast_address,
                    interface,
                    true,
                ));
                Ok(())
            }
        }

        fn leave_multicast_group(
            &self,
            multicast_address: &IpAddr,
            interface: InterfaceIndex,
        ) -> Result<(), udp::Error> {
            if self.injecting_multicast_error {
                Err(udp::Error::Syscall(
                    udp::Syscall::LeaveMulticast,
                    std::io::Error::new(std::io::ErrorKind::Other, "injected"),
                ))
            } else {
                self.mcasts.lock().unwrap().push((
                    *multicast_address,
                    interface,
                    false,
                ));
                Ok(())
            }
        }
    }

    #[derive(Default, Clone)]
    struct FakeCallback {
        calls: Arc<Mutex<Vec<Notification>>>,
    }

    impl FakeCallback {
        fn contains_notify(&self, desired_type: &str) -> bool {
            self.calls.lock().unwrap().iter().any(|n| {
                matches!(
                n,
                Notification::Alive { notification_type, .. }
                if notification_type == desired_type
                    )
            })
        }

        fn contains_byebye(&self, desired_type: &str) -> bool {
            self.calls.lock().unwrap().iter().any(|n| {
                matches!(n,
                Notification::ByeBye { notification_type, .. }
                if notification_type == desired_type
                )
            })
        }

        fn no_notifies(&self) -> bool {
            self.calls.lock().unwrap().is_empty()
        }

        fn clear(&mut self) {
            self.calls.lock().unwrap().clear();
        }
    }

    impl Callback for FakeCallback {
        fn on_notification(&self, notification: &Notification) {
            self.calls.lock().unwrap().push(notification.clone());
        }
    }

    fn multicast_dest() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(239, 255, 255, 250),
            1900,
        ))
    }

    const LOCAL_IX: InterfaceIndex = InterfaceIndex(4);
    const LOCAL_SRC: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 100, 1));
    const LOCAL_SRC_2: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 254, 33, 203));
    const MULTICAST_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250));

    fn remote_src() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, 100, 60),
            12345,
        ))
    }

    fn new_eth0_if() -> NetworkEvent {
        NetworkEvent::NewLink(
            LOCAL_IX,
            "jeth0".to_string(),
            cotton_netif::Flags::UP
                | cotton_netif::Flags::RUNNING
                | cotton_netif::Flags::MULTICAST,
        )
    }

    fn new_eth0_if_down() -> NetworkEvent {
        NetworkEvent::NewLink(
            LOCAL_IX,
            "jeth0".to_string(),
            cotton_netif::Flags::MULTICAST,
        )
    }

    fn new_eth0_if_nomulti() -> NetworkEvent {
        NetworkEvent::NewLink(
            LOCAL_IX,
            "jeth0".to_string(),
            cotton_netif::Flags::UP | cotton_netif::Flags::RUNNING,
        )
    }

    fn del_eth0() -> NetworkEvent {
        NetworkEvent::DelLink(LOCAL_IX)
    }

    const NEW_ETH0_ADDR: NetworkEvent =
        NetworkEvent::NewAddr(LOCAL_IX, LOCAL_SRC, 8);
    const NEW_ETH0_ADDR_2: NetworkEvent =
        NetworkEvent::NewAddr(LOCAL_IX, LOCAL_SRC_2, 8);
    const DEL_ETH0_ADDR: NetworkEvent =
        NetworkEvent::DelAddr(LOCAL_IX, LOCAL_SRC, 8);
    const DEL_ETH0_ADDR_2: NetworkEvent =
        NetworkEvent::DelAddr(LOCAL_IX, LOCAL_SRC_2, 8);

    const NEW_IPV6_ADDR: NetworkEvent =
        NetworkEvent::NewAddr(LOCAL_IX, IpAddr::V6(Ipv6Addr::LOCALHOST), 64);

    fn root_advert() -> Advertisement {
        Advertisement {
            notification_type: "upnp:rootdevice".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        }
    }

    fn root_advert_2() -> Advertisement {
        Advertisement {
            notification_type: "upnp:rootdevice".to_string(),
            location: "http://127.0.0.1/nested/description.xml".to_string(),
        }
    }

    #[derive(Default)]
    struct Fixture {
        e: Engine<FakeCallback>,
        c: FakeCallback,
        s: FakeSocket,
    }

    impl Fixture {
        fn new_with<F: FnMut(&mut Fixture)>(mut f: F) -> Fixture {
            let mut fixture = Fixture::default();
            f(&mut fixture);
            fixture.c.clear();
            fixture.s.clear();
            fixture
        }
    }

    /* ==== Tests for Engine ==== */

    #[test]
    fn search_sent_on_network_event_if_already_subscribed() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_search("ssdp:all"));
    }

    #[test]
    fn search_sent_on_subscribe_if_network_already_exists() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    #[test]
    fn no_search_sent_on_down_interface() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);

        assert!(f.s.no_sends());
    }

    #[test]
    fn no_search_sent_on_non_multicast_interface() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if_nomulti(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);

        assert!(f.s.no_sends());
    }

    #[test]
    fn searches_sent_on_two_ips() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 2);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC_2,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    #[test]
    fn no_search_sent_on_deleted_ips() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();
            f.e.on_network_event(&DEL_ETH0_ADDR_2, &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    #[test]
    fn search_sent_on_interface_newly_up() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    #[test]
    fn only_one_ssdpall_search_is_sent() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("upnp::Content:2".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    #[test]
    fn two_normal_searches_are_sent() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("upnp::Content:2".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 2);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "upnp::Renderer:3")
        ));
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "upnp::Content:2")
        ));
    }

    #[test]
    fn bogus_message_ignored() {
        let mut f = Fixture::default();

        f.e.on_data(&[0, 1, 2, 3, 4, 5], &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.no_sends());
    }

    #[test]
    fn notify_calls_subscriber() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
        });

        let n = FakeSocket::build_notify("upnp::Renderer:3");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(!f.c.contains_byebye("upnp::Renderer:3"));
        assert!(f.c.contains_notify("upnp::Renderer:3"));
    }

    #[test]
    fn notify_doesnt_call_subscriber() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
        });

        let n = FakeSocket::build_notify("upnp::ContentDirectory:3");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.c.no_notifies()); // not interested in this NT
    }

    #[test]
    fn response_calls_subscriber() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
        });

        let n = FakeSocket::build_response("upnp::Renderer:3");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.c.contains_notify("upnp::Renderer:3"));
    }

    #[test]
    fn response_doesnt_call_subscriber() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Media:3".to_string(), f.c.clone(), &f.s);
        });

        let n = FakeSocket::build_response("upnp::ContentDirectory:3");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.c.no_notifies()); // not interested in this NT
    }

    #[test]
    fn notify_sent_on_network_event() {
        let mut f = Fixture::new_with(|f| {
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        // Note URL has been rewritten to include the real IP address
        assert!(f.s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive { notification_type, unique_service_name, location }
                         if notification_type == "upnp:rootdevice"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
    }

    #[test]
    fn no_notify_sent_on_down_interface() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
        });

        f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);

        assert!(f.s.no_sends());
    }

    #[test]
    fn notify_sent_on_advertise() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);

        assert!(f.s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive { notification_type, unique_service_name, location }
                         if notification_type == "upnp:rootdevice"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
    }

    #[test]
    fn notify_sent_on_deadvertise() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        f.e.deadvertise("uuid:137", &f.s);

        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyByeBye { notification_type, unique_service_name }
                         if notification_type == "upnp:rootdevice"
                         && unique_service_name == "uuid:137")
        ));
    }

    #[test]
    fn no_notify_sent_on_down_interface_on_deadvertise() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if_down(), &f.s, &f.s)
                .unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        f.e.deadvertise("uuid:137", &f.s);

        assert!(f.s.no_sends());
    }

    #[test]
    fn response_sent_to_specific_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("upnp:rootdevice");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.contains_send(
            remote_src(), LOCAL_SRC,
            |m| matches!(m,
                         Message::Response { search_target, unique_service_name,
                                             location }
                         if search_target == "upnp:rootdevice"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
    }

    #[test]
    fn response_sent_to_downlevel_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise(
                "uuid:137".to_string(),
                Advertisement {
                    notification_type: "upnp::Directory:3".to_string(),
                    location: "http://127.0.0.1/description.xml".to_string(),
                },
                &f.s,
            );
        });

        let n = FakeSocket::build_search("upnp::Directory:2");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.contains_send(
            remote_src(), LOCAL_SRC,
            |m| matches!(m,
                         Message::Response { search_target, unique_service_name,
                                             location }
                         if search_target == "upnp::Directory:2"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
    }

    #[test]
    fn response_sent_to_generic_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("ssdp:all");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.contains_send(
            remote_src(), LOCAL_SRC,
            |m| matches!(m,
                         Message::Response { search_target, unique_service_name,
                                             location }
                         if search_target == "upnp:rootdevice"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
    }

    #[test]
    fn response_not_sent_to_other_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("upnp::ContentDirectory:7");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.no_sends());
    }

    #[test]
    fn byebye_calls_subscriber() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
        });

        let n = FakeSocket::build_byebye("upnp::Renderer:3");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(!f.c.contains_notify("upnp::Renderer:3"));
        assert!(f.c.contains_byebye("upnp::Renderer:3"));
    }

    /* ==== Tests for IPv4 multicast handling ==== */

    #[test]
    fn join_multicast_on_new_interface() {
        let mut f = Fixture::default();

        f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();

        assert!(f.s.mcast_count() == 1);
        assert!(f.s.contains_mcast(MULTICAST_IP, LOCAL_IX, true));
    }

    #[test]
    fn dont_join_multicast_on_repeat_interface() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();

        assert!(f.s.no_mcasts());
    }

    #[test]
    fn leave_multicast_on_interface_gone() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&del_eth0(), &f.s, &f.s).unwrap();

        assert!(f.s.mcast_count() == 1);
        assert!(f.s.contains_mcast(MULTICAST_IP, LOCAL_IX, false));
    }

    /* ==== Tests for multicast error handling ==== */

    #[test]
    fn error_join_multicast_on_new_interface() {
        let mut f = Fixture::new_with(|f| {
            f.s.inject_multicast_error(true);
        });

        assert!(f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).is_err());
    }

    #[test]
    fn error_leave_multicast_on_interface_gone() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.s.inject_multicast_error(true);
        });

        assert!(f.e.on_network_event(&del_eth0(), &f.s, &f.s).is_err());
    }

    #[test]
    fn refresh_retransmits_adverts() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
            f.e.advertise("uuid:XYZ".to_string(), root_advert_2(), &f.s);
        });

        f.e.refresh(&f.s);

        assert!(f.s.send_count() == 2);
        assert!(f.s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive { notification_type, unique_service_name, location }
                         if notification_type == "upnp:rootdevice"
                         && unique_service_name == "uuid:137"
                         && location == "http://192.168.100.1/description.xml")));
        assert!(f.s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive { notification_type, unique_service_name, location }
                         if notification_type == "upnp:rootdevice"
                         && unique_service_name == "uuid:XYZ"
                         && location == "http://192.168.100.1/nested/description.xml")));
    }

    #[test]
    fn refresh_retransmits_searches() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("upnp::Content:2".to_string(), f.c.clone(), &f.s);
        });

        f.e.refresh(&f.s);

        assert!(f.s.send_count() == 2);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "upnp::Renderer:3")
        ));
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "upnp::Content:2")
        ));
    }

    #[test]
    fn refresh_retransmits_generic_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
        });

        f.e.refresh(&f.s);

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(
            multicast_dest(),
            LOCAL_SRC,
            |m| matches!(m,
                         Message::Search { search_target, .. }
                         if search_target == "ssdp:all")
        ));
    }

    /* ==== Tests for out-of-sequence messages ==== */

    #[test]
    fn bogus_dellink_ignored() {
        let mut f = Fixture::default();

        f.e.on_network_event(&del_eth0(), &f.s, &f.s).unwrap();
    }

    #[test]
    fn repeat_address_ignored() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_sends());
    }

    #[test]
    fn address_before_link_ignored() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
        });

        f.e.on_network_event(&NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_sends());
    }

    #[test]
    fn ipv6_address_ignored() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&NEW_IPV6_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_sends());
    }

    #[test]
    fn bogus_deladdr_ignored() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_network_event(&new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_network_event(&DEL_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_sends());
    }

    #[test]
    fn bogus_deladdr_ignored_2() {
        let mut f = Fixture::default();

        f.e.on_network_event(&DEL_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_sends());
    }

    #[test]
    fn bogus_deadvertise_ignored() {
        let mut f = Fixture::default();

        f.e.deadvertise("uuid:137", &f.s);

        assert!(f.s.no_sends());
    }

    #[test]
    fn url_host_rewritten() {
        let url = rewrite_host("http://127.0.0.1/description.xml", &LOCAL_SRC);
        assert_eq!(url, "http://192.168.100.1/description.xml");
    }

    #[test]
    fn url_host_rewritten2() {
        let url = rewrite_host("http://127.0.0.1/", &LOCAL_SRC);
        assert_eq!(url, "http://192.168.100.1/");
    }

    #[test]
    fn url_host_rewritten3() {
        let url = rewrite_host("http://127.0.0.1", &LOCAL_SRC);
        assert_eq!(url, "http://192.168.100.1");
    }

    #[test]
    fn url_host_rewritten4() {
        let url = rewrite_host("http://127.0.0.1:3333/foo/bar", &LOCAL_SRC);
        assert_eq!(url, "http://192.168.100.1:3333/foo/bar");
    }

    #[test]
    fn bogus_url_passed_through() {
        let url = rewrite_host("fnord", &LOCAL_SRC);
        assert_eq!(url, "fnord".to_string());
    }

    #[test]
    fn bogus_url_passed_through2() {
        let url = rewrite_host("fnord:/", &LOCAL_SRC);
        assert_eq!(url, "fnord:/".to_string());
    }
}
