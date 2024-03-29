use cotton_ssdp::{Notification, Service};
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;

const SSDP_TOKEN1: mio::Token = mio::Token(0);
const SSDP_TOKEN2: mio::Token = mio::Token(1);

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "ssdp-search-mio from {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let mut poll = mio::Poll::new()?;
    let mut events = mio::Events::with_capacity(128);

    let mut ssdp = Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2))?;

    let uuid = uuid::Uuid::new_v4();
    ssdp.advertise(
        uuid.to_string(),
        cotton_ssdp::Advertisement {
            notification_type: "test".to_string(),
            location: "http://127.0.0.1/test".to_string(),
        },
    );

    let map = RefCell::new(HashMap::new());
    ssdp.subscribe(
        "ssdp:all",
        Box::new(move |r| {
            println!("GOT {r:?}");
            if let Notification::Alive {
                ref notification_type,
                ref unique_service_name,
                ref location,
            } = r
            {
                let mut m = map.borrow_mut();
                if !m.contains_key(unique_service_name) {
                    println!("+ {notification_type}");
                    println!("  {unique_service_name} at {location}");
                    m.insert(unique_service_name.clone(), r.clone());
                }
            }
        }),
    );

    loop {
        poll.poll(&mut events, Some(ssdp.next_wakeup())).unwrap();

        if ssdp.next_wakeup() == std::time::Duration::ZERO {
            // Timeout
            ssdp.wakeup();
        }

        for event in &events {
            match event.token() {
                SSDP_TOKEN1 => ssdp.multicast_ready(),
                SSDP_TOKEN2 => ssdp.search_ready(),
                _ => (),
            }
        }
    }
}
