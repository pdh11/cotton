use cotton_ssdp::Service;
use serial_test::*;
use std::cell::RefCell;
use std::panic;
use std::rc::Rc;
use std::sync::atomic::{self, AtomicBool};
use std::thread;
use std::time::Duration;
use systemtests::{device_test, DeviceTest};

fn nucleo_test<F: FnOnce(DeviceTest) -> () + panic::UnwindSafe>(
    firmware: &str,
    f: F,
) {
    device_test(
        "STM32F746ZGTx",
        "COTTON_PROBE_STM32F746_NUCLEO",
        firmware,
        f,
    );
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_hello() {
    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/hello",
        |mut t| {
            t.expect("Hello STM32F746 Nucleo", Duration::from_secs(5));
        },
    );
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_dhcp() {
    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/dhcp-rtic",
        |mut t| {
            t.expect_stderr("(HOST) INFO  success!", Duration::from_secs(30));
            t.expect("DHCP config acquired!", Duration::from_secs(10));
        },
    );
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_ssdp() {
    const SSDP_TOKEN1: mio::Token = mio::Token(0);
    const SSDP_TOKEN2: mio::Token = mio::Token(1);

    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/ssdp-rtic",
        |mut t| {
            // Set to true once the device has seen our advertisement
            let seen_server = AtomicBool::new(false);
            t.expect_stderr("success!", Duration::from_secs(30));
            t.expect("DHCP config acquired!", Duration::from_secs(10));

            thread::scope(|s| {
                s.spawn(|| {
                    // Set to true once we've seen the device's advertisement
                    let seen_client = Rc::new(RefCell::new(false));
                    let seen_client2 = seen_client.clone();
                    let mut poll = mio::Poll::new().unwrap();
                    let mut events = mio::Events::with_capacity(128);

                    let mut ssdp = Service::new(
                        poll.registry(),
                        (SSDP_TOKEN1, SSDP_TOKEN2),
                    )
                    .unwrap();

                    let uuid = uuid::Uuid::new_v4();
                    ssdp.advertise(
                        uuid.to_string(),
                        cotton_ssdp::Advertisement {
                            notification_type: "cotton-test-server"
                                .to_string(),
                            location: "http://127.0.0.1/test".to_string(),
                        },
                    );

                    ssdp.subscribe(
                        "ssdp:all",
                        Box::new(move |r| {
                            println!("HOST GOT {r:?}");
                            if let cotton_ssdp::Notification::Alive {
                                ref notification_type,
                                ..
                            } = r
                            {
                                if notification_type == "stm32f746-nucleo-test"
                                {
                                    *seen_client.borrow_mut() = true;
                                }
                            }
                        }),
                    );

                    loop {
                        poll.poll(&mut events, Some(Duration::from_secs(1)))
                            .unwrap();

                        if *seen_client2.borrow()
                            && seen_server.load(atomic::Ordering::Acquire)
                        {
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
                t.expect("SSDP! cotton-test-server", Duration::from_secs(20));
                seen_server.store(true, atomic::Ordering::Release);
            });
        },
    );
}
