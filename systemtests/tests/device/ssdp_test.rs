use assertables::*;
use cotton_ssdp::Service;
use std::collections::HashSet;
use std::panic;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct SsdpTest {
    seen: Arc<Mutex<HashSet<String>>>,
}

impl SsdpTest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn expect_seen(&self, notification_type: &str, timeout: Duration) {
        let start = Instant::now();
        eprintln!("{:?}: Looking for {notification_type}", Instant::now());

        loop {
            {
                let v = self.seen.lock().unwrap();
                if v.contains(notification_type) {
                    return;
                }
                if start.elapsed() > timeout {
                    eprintln!("{:?}: Didn't find it", Instant::now());
                    assert_contains!(v, notification_type);
                    return;
                }
                // drop the lock
            }
            sleep(Duration::from_millis(200));
        }
    }
}

pub fn ssdp_test<F: FnOnce(SsdpTest) -> () + panic::UnwindSafe>(
    my_service: &'static str,
    device_service: &'static str,
    f: F,
) {
    let t = SsdpTest::new();
    let mut result = Ok(());
    let done = AtomicBool::new(false);
    let seen2 = t.seen.clone();

    thread::scope(|s| {
        s.spawn(|| {
            const SSDP_TOKEN1: mio::Token = mio::Token(0);
            const SSDP_TOKEN2: mio::Token = mio::Token(1);
            let mut poll = mio::Poll::new().unwrap();
            let mut events = mio::Events::with_capacity(128);

            let mut ssdp =
                Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2))
                    .unwrap();

            let uuid = uuid::Uuid::new_v4();
            ssdp.advertise(
                uuid.to_string(),
                cotton_ssdp::Advertisement {
                    notification_type: my_service.to_string(),
                    location: "http://127.0.0.1/test".to_string(),
                },
            );

            ssdp.subscribe(
                device_service,
                Box::new(move |r| {
                    println!("HOST GOT {r:?}");
                    if let cotton_ssdp::Notification::Alive {
                        notification_type,
                        ..
                    } = r
                    {
                        let mut v = seen2.lock().unwrap();
                        v.insert(notification_type.clone());
                    }
                }),
            );

            loop {
                poll.poll(&mut events, Some(Duration::from_secs(1)))
                    .unwrap();

                if done.load(atomic::Ordering::Acquire) {
                    return;
                }

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
        });
        result = panic::catch_unwind(|| f(t));
        done.store(true, atomic::Ordering::Release);
    });
    assert!(result.is_ok());
}
