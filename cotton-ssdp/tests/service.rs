use cotton_ssdp::{Advertisement, Notification, Service};
use serial_test::*;
use std::cell::RefCell;
use std::rc::Rc;

// "PowerPC" here really means "using QEMU", where
// IP_{ADD/DEL}_MEMBERSHIP fail mysteriously
// https://gitlab.com/qemu-project/qemu/-/issues/2553
#[test]
#[serial(ssdp)]
#[cfg_attr(miri, ignore)]
#[cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))]
fn services_can_communicate_notify() {
    const SSDP_TOKEN1: mio::Token = mio::Token(1);
    const SSDP_TOKEN2: mio::Token = mio::Token(2);
    const SSDP_TOKEN3: mio::Token = mio::Token(3);
    const SSDP_TOKEN4: mio::Token = mio::Token(4);
    let mut poll = mio::Poll::new().unwrap();
    let mut ssdp1 =
        Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
    let mut ssdp2 =
        Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Fnord:3".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();

    ssdp2.subscribe(
        "upnp::Fnord:3",
        Box::new(move |r| {
            seen2.borrow_mut().push(r.clone());
        }),
    );

    let mut events = mio::Events::with_capacity(1024);
    while !seen.borrow().iter().any(|r| {
        matches!(r,
                 Notification::Alive { notification_type, unique_service_name, .. } if
                 notification_type == "upnp::Fnord:3"
                 && unique_service_name == "uuid:999"
        )
    }) {
        poll.poll(&mut events,
                  Some(ssdp1.next_wakeup().min(ssdp2.next_wakeup())))
            .unwrap();

        ssdp1.wakeup();
        ssdp2.wakeup();

        // Has SSDP2 seen SSDP1's *multicast* notify?

        for _ in &events {
            // We could tell, from event.token, which socket is
            // readable. But as this is a test, for coverage
            // purposes we always check everything.
            ssdp1.multicast_ready();
            ssdp1.search_ready();
            ssdp2.multicast_ready();
            //ssdp2.search_ready();
        }
    }
}

#[test]
#[serial(ssdp)]
#[cfg_attr(miri, ignore)]
#[cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))]
fn services_can_communicate_search() {
    const SSDP_TOKEN1: mio::Token = mio::Token(1);
    const SSDP_TOKEN2: mio::Token = mio::Token(2);
    const SSDP_TOKEN3: mio::Token = mio::Token(3);
    const SSDP_TOKEN4: mio::Token = mio::Token(4);
    let mut poll = mio::Poll::new().unwrap();
    let mut ssdp1 =
        Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
    let mut ssdp2 =
        Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Directory:3".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();

    ssdp2.subscribe(
        "upnp::Directory:3",
        Box::new(move |r| {
            seen2.borrow_mut().push(r.clone());
        }),
    );

    let mut events = mio::Events::with_capacity(1024);
    while !seen.borrow().iter().any(|r| {
        matches!(r,
        Notification::Alive { notification_type, unique_service_name, .. }
        if notification_type == "upnp::Directory:3"
        && unique_service_name == "uuid:999"
        )
    }) {
        let sleep = ssdp1.next_wakeup().min(ssdp2.next_wakeup());
        println!("polling");
        poll.poll(&mut events, Some(sleep)).unwrap();
        println!("polled");

        ssdp1.wakeup();
        ssdp2.wakeup();

        // Has SSDP2 seen SSDP1's *unicast* reply?

        for _ in &events {
            ssdp1.multicast_ready();
            ssdp1.search_ready();
            //ssdp2.multicast_ready();
            ssdp2.search_ready();
        }
    }
}

#[test]
#[serial(ssdp)]
#[cfg_attr(miri, ignore)]
#[cfg(not(any(target_arch = "powerpc", target_arch = "powerpc64")))]
fn services_can_deadvertise() {
    const SSDP_TOKEN1: mio::Token = mio::Token(1);
    const SSDP_TOKEN2: mio::Token = mio::Token(2);
    const SSDP_TOKEN3: mio::Token = mio::Token(3);
    const SSDP_TOKEN4: mio::Token = mio::Token(4);
    let mut poll = mio::Poll::new().unwrap();
    let mut ssdp1 =
        Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();
    let mut ssdp2 =
        Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();

    ssdp1.advertise(
        "uuid:998",
        Advertisement {
            notification_type: "upnp::Directory:4".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();

    ssdp2.subscribe(
        "upnp::Directory:4",
        Box::new(move |r| {
            seen2.borrow_mut().push(r.clone());
        }),
    );

    ssdp1.deadvertise("uuid:998");

    let mut events = mio::Events::with_capacity(1024);
    while !seen.borrow().iter().any(|r| {
        matches!(r,
                 Notification::ByeBye { notification_type, unique_service_name, .. } if
                 notification_type == "upnp::Directory:4"
                 && unique_service_name == "uuid:998"
        )
    }) {
        poll.poll(&mut events,
                  Some(ssdp1.next_wakeup().min(ssdp2.next_wakeup())))
            .unwrap();

        ssdp1.wakeup();
        ssdp2.wakeup();

        for _ in &events {
            // We could tell, from event.token, which socket is
            // readable. But as this is a test, for coverage
            // purposes we always check everything.
            ssdp1.multicast_ready();
            ssdp1.search_ready();
            ssdp2.multicast_ready();
            ssdp2.search_ready();
        }
    }
}
