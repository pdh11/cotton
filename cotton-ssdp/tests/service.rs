use cotton_ssdp::*;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
#[cfg_attr(miri, ignore)]
fn services_can_communicate() {
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
                 Notification::Alive { notification_type, unique_service_name, .. } if
                 notification_type == "upnp::Directory:3"
                 && unique_service_name == "uuid:999"
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

#[test]
#[cfg_attr(miri, ignore)]
fn services_can_communicate_unicast() {
    const SSDP_TOKEN1: mio::Token = mio::Token(1);
    const SSDP_TOKEN2: mio::Token = mio::Token(2);
    const SSDP_TOKEN3: mio::Token = mio::Token(3);
    const SSDP_TOKEN4: mio::Token = mio::Token(4);

    let mut poll = mio::Poll::new().unwrap();
    let mut ssdp1 =
        Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2)).unwrap();

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Directory:3".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    let mut ssdp2 =
        Service::new(poll.registry(), (SSDP_TOKEN3, SSDP_TOKEN4)).unwrap();

    // Get initial NOTIFY out of the way
    let mut events = mio::Events::with_capacity(1024);
    loop {
        poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))
            .unwrap();
        if events.is_empty() {
            break;
        }

        // We could tell, from event.token, which socket is readable. But
        // as this is a test, for coverage purposes we always check
        // everything.
        for _ in &events {
            ssdp1.multicast_ready();
            ssdp1.search_ready();
        }
    }

    let seen = Rc::new(RefCell::new(Vec::new()));

    // ssdp1's initial NOTIFY has already happened, so the only way we'll
    // find it here is if searching (with unicast reply) also works.
    let seen2 = seen.clone();
    ssdp2.subscribe(
        "upnp::Directory:3",
        Box::new(move |r| {
            seen2.borrow_mut().push(r.clone());
        }),
    );

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

        //ssdp1.wakeup();
        ssdp2.wakeup();
        ssdp1.multicast_ready();
        ssdp1.multicast_ready();

        for _ in &events {
            ssdp1.multicast_ready();
            ssdp1.search_ready();
            ssdp2.multicast_ready();
            ssdp2.search_ready();
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)]
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
