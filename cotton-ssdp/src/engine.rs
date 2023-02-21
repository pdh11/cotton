use crate::message;
use crate::message::Message;
use crate::udp;
use crate::{Advertisement, Notification, NotificationSubtype};
use cotton_netif::{InterfaceIndex, NetworkEvent};
use rand::Rng;
use slotmap::SlotMap;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

const MAX_PACKET_SIZE: usize = 512;

pub struct Interface {
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
                        return cversion <= sversion;
                    }
                }
            }
        }
    }
    false
}

pub trait Callback {
    fn on_notification(&self, notification: &Notification);
}

struct ActiveSearch<CB: Callback> {
    notification_type: String,
    callback: CB,
}

slotmap::new_key_type! { struct ActiveSearchKey; }

pub struct Engine<CB: Callback> {
    interfaces: HashMap<InterfaceIndex, Interface>,
    active_searches: SlotMap<ActiveSearchKey, ActiveSearch<CB>>,
    advertisements: HashMap<String, Advertisement>,
    next_salvo: std::time::Instant,
    phase: u8,
}

impl<CB: Callback> Default for Engine<CB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<CB: Callback> Engine<CB> {
    #[must_use]
    pub fn new() -> Self {
        Engine {
            interfaces: HashMap::default(),
            active_searches: SlotMap::with_key(),
            advertisements: HashMap::default(),
            next_salvo: std::time::Instant::now(),
            phase: 0u8,
        }
    }

    #[must_use]
    pub fn next_wakeup(&self) -> std::time::Duration {
        self.next_salvo
            .saturating_duration_since(std::time::Instant::now())
    }

    pub fn wakeup<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        socket: &SCK,
    ) {
        if !self.next_wakeup().is_zero() {
            return;
        }
        let random_offset = rand::thread_rng().gen_range(0..5);
        let period_sec = if self.phase == 0 { 800 } else { 1 } + random_offset;
        self.next_salvo += Duration::from_secs(period_sec);
        self.phase = (self.phase + 1) % 4;

        println!(
            "Re-advertising, re-searching, next wu at {:?} phase {}\n",
            self.next_salvo, self.phase
        );

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
        search_type: &String,
        socket: &SCK,
    ) {
        println!("search_on_all({})", search_type);

        for interface in self.interfaces.values() {
            for ip in &interface.ips {
                if ip.is_ipv4() {
                    Self::search_on(search_type, ip, socket);
                }
            }
        }
    }

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
            if target_match(
                &s.notification_type,
                &notification.notification_type,
            ) {
                s.callback.on_notification(notification);
            }
        }
    }

    pub fn on_data<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        buf: &[u8],
        socket: &SCK,
        wasto: IpAddr,
        wasfrom: SocketAddr,
    ) {
        if let Ok(m) = message::parse(buf) {
            match m {
                Message::NotifyAlive(a) => {
                    self.call_subscribers(&Notification {
                        notification_type: a.notification_type,
                        unique_service_name: a.unique_service_name,
                        notification_subtype:
                            NotificationSubtype::AliveLocation(a.location),
                    });
                }
                Message::NotifyByeBye(a) => {
                    self.call_subscribers(&Notification {
                        notification_type: a.notification_type,
                        unique_service_name: a.unique_service_name,
                        notification_subtype: NotificationSubtype::ByeBye,
                    });
                }
                Message::Search(s) => {
                    println!("Got search for {}", s.search_target);
                    for (key, value) in &self.advertisements {
                        if target_match(
                            &s.search_target,
                            &value.notification_type,
                        ) {
                            println!(
                                "  {} matches, replying",
                                value.notification_type
                            );
                            let mut url = value.location.clone();
                            let _ = url.set_ip_host(wasto);

                            let _ = socket.send_with(
                                MAX_PACKET_SIZE,
                                &wasfrom,
                                &wasto,
                                |b| {
                                    message::build_response(
                                        b,
                                        &value.notification_type,
                                        key,
                                        url.as_str(),
                                    )
                                },
                            );
                        }
                    }
                }
                Message::Response(r) => self.call_subscribers(&Notification {
                    notification_type: r.search_target,
                    unique_service_name: r.unique_service_name,
                    notification_subtype: NotificationSubtype::AliveLocation(
                        r.location,
                    ),
                }),
            };
        }
    }

    fn join_multicast<SCK: udp::TargetedSend + udp::Multicast>(
        ip: &IpAddr,
        multicast: &SCK,
    ) -> Result<(), std::io::Error> {
        if ip.is_ipv4() {
            multicast
                .join_multicast_group(&"239.255.255.250".parse().unwrap(), ip)
        } else {
            Ok(())
        }
    }

    fn leave_multicast<SCK: udp::TargetedSend + udp::Multicast>(
        ip: &IpAddr,
        multicast: &SCK,
    ) -> Result<(), std::io::Error> {
        if ip.is_ipv4() {
            multicast
                .leave_multicast_group(&"239.255.255.250".parse().unwrap(), ip)
        } else {
            Ok(())
        }
    }

    fn send_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        ips: &[IpAddr],
        search: &SCK,
    ) {
        for ip in ips {
            if ip.is_ipv4() {
                println!("Searching on {:?}", ip);
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
    }

    pub fn on_interface_event<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        e: NetworkEvent,
        multicast: &SCK,
        search: &SCK,
    ) -> Result<(), std::io::Error> {
        println!("if event {:?}", e);
        match e {
            NetworkEvent::NewLink(ix, _name, flags) => {
                let up = flags.contains(
                    cotton_netif::Flags::RUNNING
                        | cotton_netif::Flags::UP
                        | cotton_netif::Flags::MULTICAST,
                );
                let mut new_ix = None;
                if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                    if up && !v.up {
                        if let Some(ip) = v.ips.get(0) {
                            Self::join_multicast(&ip, multicast)?;
                        }
                        new_ix = Some(ix);
                    } else if !up && v.up {
                        if let Some(ip) = v.ips.get(0) {
                            Self::leave_multicast(&ip, multicast)?;
                        }
                    }
                    v.up = up;
                } else {
                    self.interfaces.insert(
                        ix,
                        Interface {
                            ips: Vec::new(),
                            up,
                        },
                    );
                }
                if let Some(ix) = new_ix {
                    if let Some(v) = self.interfaces.get(&ix) {
                        self.send_all(&v.ips, search);
                    }
                }
            }
            NetworkEvent::DelLink(ix) => {
                if let Some(ref v) = self.interfaces.remove(&ix) {
                    if v.up {
                        if let Some(ip) = v.ips.get(0) {
                            Self::leave_multicast(&ip, multicast)?;
                        }
                    }
                }
            }
            NetworkEvent::NewAddr(ix, addr, _prefix) => {
                if addr.is_ipv4() {
                    if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                        if !v.ips.contains(&addr) {
                            if v.ips.is_empty() {
                                Self::join_multicast(&addr, multicast)?;
                            }
                            v.ips.push(addr);
                            if v.up {
                                self.send_all(&[addr], search);
                            }
                        }
                    } else {
                        self.interfaces.insert(
                            ix,
                            Interface {
                                ips: vec![addr],
                                up: false,
                            },
                        );
                    }
                }
            }
            NetworkEvent::DelAddr(ix, addr, _prefix) => {
                if let Some(ref mut v) = self.interfaces.get_mut(&ix) {
                    if let Some(n) = v.ips.iter().position(|&a| a == addr) {
                        v.ips.swap_remove(n);
                        println!("Found IP, removed up={}, ips now {:?}",
                                 v.up, v.ips);
                        if v.up && v.ips.is_empty() {
                            Self::leave_multicast(&addr, multicast)?;
                        }
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
        let mut url = advertisement.location.clone();
        let _ = url.set_ip_host(*source);
        let _ = socket.send_with(
            MAX_PACKET_SIZE,
            &"239.255.255.250:1900".parse().unwrap(),
            source,
            |b| {
                message::build_notify(
                    b,
                    &advertisement.notification_type,
                    unique_service_name,
                    url.as_str(),
                )
            },
        );
        println!("Advertising {:?} from {:?}", url, source);
    }

    fn notify_on_all<SCK: udp::TargetedSend + udp::Multicast>(
        &self,
        unique_service_name: &str,
        advertisement: &Advertisement,
        socket: &SCK,
    ) {
        for interface in self.interfaces.values() {
            for ip in &interface.ips {
                if ip.is_ipv4() {
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

    pub fn advertise<SCK: udp::TargetedSend + udp::Multicast>(
        &mut self,
        unique_service_name: String,
        advertisement: Advertisement,
        socket: &SCK,
    ) {
        println!("Advertising {}", unique_service_name);
        self.notify_on_all(&unique_service_name, &advertisement, socket);
        self.advertisements
            .insert(unique_service_name, advertisement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::parse;
    use std::net::{Ipv4Addr, SocketAddrV4};
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
        // If we search for CD:2 we should pick up CD:1's, but not vice versa
        assert!(target_match(
            "upnp::ContentDirectory:2",
            "upnp::ContentDirectory:1"
        ));
        assert!(!target_match(
            "upnp::ContentDirectory:1",
            "upnp::ContentDirectory:2"
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
        mcasts: Mutex<Vec<(IpAddr, IpAddr, bool)>>,
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
                *to == wasto && *from == wasfrom && f(&msg)
            })
        }

        fn no_sends(&self) -> bool {
            self.sends.lock().unwrap().is_empty()
        }

        fn send_count(&self) -> usize {
            self.sends.lock().unwrap().len()
        }

        fn contains_mcast(&self, group: IpAddr, host: IpAddr, join: bool) -> bool {
            self.mcasts.lock().unwrap().iter().any(|(gp, hst, jn)| {
                *gp == group && *hst == host && *jn == join
            })
        }

        fn no_mcasts(&self) -> bool {
            self.mcasts.lock().unwrap().is_empty()
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
    }

    impl udp::TargetedSend for FakeSocket {
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
            my_address: &IpAddr,
        ) -> Result<(), std::io::Error> {
            self.mcasts.lock().unwrap().push((
                *multicast_address,
                *my_address,
                true,
            ));
            Ok(())
        }

        fn leave_multicast_group(
            &self,
            multicast_address: &IpAddr,
            my_address: &IpAddr,
        ) -> Result<(), std::io::Error> {
            self.mcasts.lock().unwrap().push((
                *multicast_address,
                *my_address,
                false,
            ));
            Ok(())
        }
    }

    #[derive(Default, Clone)]
    struct FakeCallback {
        calls: Arc<Mutex<Vec<Notification>>>,
    }

    impl FakeCallback {
        fn contains_notify(&self, notification_type: &str) -> bool {
            self.calls.lock().unwrap().iter().any(|n| {
                n.notification_type == notification_type
                    && matches!(
                        n.notification_subtype,
                        NotificationSubtype::AliveLocation(_)
                    )
            })
        }

        fn contains_byebye(&self, notification_type: &str) -> bool {
            self.calls.lock().unwrap().iter().any(|n| {
                n.notification_type == notification_type
                    && matches!(
                        n.notification_subtype,
                        NotificationSubtype::ByeBye
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
            InterfaceIndex(4),
            "jeth0".to_string(),
            cotton_netif::Flags::UP
                | cotton_netif::Flags::RUNNING
                | cotton_netif::Flags::MULTICAST,
        )
    }

    const NEW_ETH0_ADDR: NetworkEvent =
        NetworkEvent::NewAddr(InterfaceIndex(4), LOCAL_SRC, 8);
    const NEW_ETH0_ADDR_2: NetworkEvent =
        NetworkEvent::NewAddr(InterfaceIndex(4), LOCAL_SRC_2, 8);
    const DEL_ETH0_ADDR: NetworkEvent =
        NetworkEvent::DelAddr(InterfaceIndex(4), LOCAL_SRC, 8);
    const DEL_ETH0_ADDR_2: NetworkEvent =
        NetworkEvent::DelAddr(InterfaceIndex(4), LOCAL_SRC_2, 8);

    fn root_advert() -> Advertisement {
        Advertisement {
            notification_type: "upnp:rootdevice".to_string(),
            location: url::Url::parse("http://127.0.0.1/description.xml")
                .unwrap(),
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
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.contains_send(multicast_dest(), LOCAL_SRC, |m| matches!(m,
                         Message::Search(s)
                         if s.search_target == "ssdp:all")));
    }

    #[test]
    fn search_sent_on_subscribe_if_network_already_exists() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);

        assert!(f.s.contains_send(multicast_dest(), LOCAL_SRC, |m| matches!(m,
                         Message::Search(s)
                         if s.search_target == "ssdp:all")));
    }

    #[test]
    fn only_one_ssdpall_search_is_sent() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("ssdp:all".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("upnp::Content:2".to_string(), f.c.clone(), &f.s);
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 1);
        assert!(f.s.contains_send(multicast_dest(), LOCAL_SRC, |m| matches!(m,
                         Message::Search(s)
                         if s.search_target == "ssdp:all")));
    }

    #[test]
    fn two_normal_searches_are_sent() {
        let mut f = Fixture::new_with(|f| {
            f.e.subscribe("upnp::Renderer:3".to_string(), f.c.clone(), &f.s);
            f.e.subscribe("upnp::Content:2".to_string(), f.c.clone(), &f.s);
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.send_count() == 2);
        assert!(f.s.contains_send(multicast_dest(), LOCAL_SRC, |m| matches!(m,
                         Message::Search(s)
                         if s.search_target == "upnp::Renderer:3")));
        assert!(f.s.contains_send(multicast_dest(), LOCAL_SRC, |m| matches!(m,
                         Message::Search(s)
                         if s.search_target == "upnp::Content:2")));
    }

    #[test]
    fn bogus_message_ignored() {
        let mut f = Fixture::default();

        f.e.on_data(&[0, 1, 2, 3, 4, 5], &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.no_sends());
    }

    #[test]
    fn notify_calls_subscriber() {
        let mut e = Engine::<FakeCallback>::new();
        let c = FakeCallback::default();
        let s = FakeSocket::default();

        e.subscribe("upnp::MediaRenderer:3".to_string(), c.clone(), &s);

        assert!(c.no_notifies());

        let n = FakeSocket::build_notify("upnp::MediaRenderer:3");
        e.on_data(&n, &s, LOCAL_SRC, remote_src());

        assert!(c.contains_notify("upnp::MediaRenderer:3"));
    }

    #[test]
    fn notify_doesnt_call_subscriber() {
        let mut e = Engine::<FakeCallback>::new();
        let c = FakeCallback::default();
        let s = FakeSocket::default();

        e.subscribe("upnp::MediaRenderer:3".to_string(), c.clone(), &s);

        assert!(c.no_notifies());

        let n = FakeSocket::build_notify("upnp::ContentDirectory:3");
        e.on_data(&n, &s, LOCAL_SRC, remote_src());

        assert!(c.no_notifies()); // not interested in this NT
    }

    #[test]
    fn response_calls_subscriber() {
        let mut e = Engine::<FakeCallback>::new();
        let c = FakeCallback::default();
        let s = FakeSocket::default();

        e.subscribe("upnp::MediaRenderer:3".to_string(), c.clone(), &s);

        assert!(c.no_notifies());

        let n = FakeSocket::build_response("upnp::MediaRenderer:3");
        e.on_data(&n, &s, LOCAL_SRC, remote_src());

        assert!(c.contains_notify("upnp::MediaRenderer:3"));
    }

    #[test]
    fn response_doesnt_call_subscriber() {
        let mut e = Engine::<FakeCallback>::new();
        let c = FakeCallback::default();
        let s = FakeSocket::default();

        e.subscribe("upnp::MediaRenderer:3".to_string(), c.clone(), &s);

        assert!(c.no_notifies());

        let n = FakeSocket::build_response("upnp::ContentDirectory:3");
        e.on_data(&n, &s, LOCAL_SRC, remote_src());

        assert!(c.no_notifies()); // not interested in this NT
    }

    #[test]
    fn notify_sent_on_network_event() {
        let mut e = Engine::<FakeCallback>::default();
        let s = FakeSocket::default();

        e.advertise("uuid:137".to_string(), root_advert(), &s);

        assert!(s.no_sends());

        e.on_interface_event(new_eth0_if(), &s, &s).unwrap();

        assert!(s.no_sends());

        e.on_interface_event(NEW_ETH0_ADDR, &s, &s).unwrap();

        // Note URL has been rewritten to include the real IP address
        assert!(s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive(s)
                         if s.notification_type == "upnp:rootdevice"
                         && s.unique_service_name == "uuid:137"
                         && s.location == "http://192.168.100.1/description.xml")))
    }

    #[test]
    fn notify_sent_on_advertise() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);

        assert!(f.s.contains_send(
            multicast_dest(), LOCAL_SRC,
            |m| matches!(m,
                         Message::NotifyAlive(s)
                         if s.notification_type == "upnp:rootdevice"
                         && s.unique_service_name == "uuid:137"
                         && s.location == "http://192.168.100.1/description.xml")))
    }

    #[test]
    fn response_sent_to_specific_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("upnp:rootdevice");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.contains_send(
            remote_src(), LOCAL_SRC,
            |m| matches!(m,
                         Message::Response(s)
                         if s.search_target == "upnp:rootdevice"
                         && s.unique_service_name == "uuid:137"
                         && s.location == "http://192.168.100.1/description.xml")))
    }

    #[test]
    fn response_sent_to_generic_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("ssdp:all");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.contains_send(
            remote_src(), LOCAL_SRC,
            |m| matches!(m,
                         Message::Response(s)
                         if s.search_target == "upnp:rootdevice"
                         && s.unique_service_name == "uuid:137"
                         && s.location == "http://192.168.100.1/description.xml")))
    }

    #[test]
    fn response_not_sent_to_other_search() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.advertise("uuid:137".to_string(), root_advert(), &f.s);
        });

        let n = FakeSocket::build_search("upnp::ContentDirectory:7");
        f.e.on_data(&n, &f.s, LOCAL_SRC, remote_src());

        assert!(f.s.no_sends());
    }

    /* ==== Tests for IPv4 multicast handling ==== */

    #[test]
    fn join_multicast_on_first_ip() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.contains_mcast(MULTICAST_IP, LOCAL_SRC, true));
    }

    #[test]
    fn dont_rejoin_multicast_on_second_ip() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();

        assert!(f.s.no_mcasts());
    }

    #[test]
    fn leave_multicast_on_losing_only_ip() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(DEL_ETH0_ADDR, &f.s, &f.s).unwrap();

        // Wait, does this even work? How does the kernel know which
        // interface we're trying to leave multicast on, if the
        // address has already gone away?

        assert!(f.s.contains_mcast(MULTICAST_IP, LOCAL_SRC, false));
    }

    #[test]
    fn dont_leave_multicast_on_losing_first_ip() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(DEL_ETH0_ADDR, &f.s, &f.s).unwrap();

        assert!(f.s.no_mcasts());
    }

    #[test]
    fn dont_leave_multicast_on_losing_second_ip() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(DEL_ETH0_ADDR_2, &f.s, &f.s).unwrap();

        assert!(f.s.no_mcasts());
    }

    #[test]
    fn leave_multicast_on_losing_both_ips() {
        let mut f = Fixture::new_with(|f| {
            f.e.on_interface_event(new_eth0_if(), &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR, &f.s, &f.s).unwrap();
            f.e.on_interface_event(NEW_ETH0_ADDR_2, &f.s, &f.s).unwrap();
            f.e.on_interface_event(DEL_ETH0_ADDR, &f.s, &f.s).unwrap();
        });

        f.e.on_interface_event(DEL_ETH0_ADDR_2, &f.s, &f.s).unwrap();

        assert!(f.s.contains_mcast(MULTICAST_IP, LOCAL_SRC_2, false));
    }
}
