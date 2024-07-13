use crate::device_test::{device_test, DeviceTest};
use crate::ssdp_test::ssdp_test;
use serial_test::*;
use std::panic;
use std::time::Duration;

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
#[serial(stm32f746_nucleo)]
#[cfg_attr(miri, ignore)]
fn arm_stm32f746_nucleo_0hello() {
    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/stm32f746-nucleo-hello",
        |t| {
            t.expect("Hello STM32F746 Nucleo", Duration::from_secs(25));
        },
    );
}

#[test]
#[serial(stm32f746_nucleo)]
#[cfg_attr(miri, ignore)]
fn arm_stm32f746_nucleo_dhcp() {
    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/stm32f746-nucleo-dhcp-rtic",
        |t| {
            t.expect_stderr("Finished in", Duration::from_secs(45));
            t.expect("DHCP config acquired!", Duration::from_secs(10));
        },
    );
}

#[test]
#[serial(stm32f746_nucleo)]
#[cfg_attr(miri, ignore)]
fn arm_stm32f746_nucleo_ssdp() {
    nucleo_test(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/stm32f746-nucleo-ssdp-rtic",
        |nt| {
            nt.expect_stderr("Finished in", Duration::from_secs(45));
            nt.expect("DHCP config acquired!", Duration::from_secs(10));
            ssdp_test(
                "cotton-test-server-stm32f746",  // host service
                "stm32f746-nucleo-test", // device service
                |st| {
                    nt.expect("SSDP! cotton-test-server-stm32f746",
                              Duration::from_secs(20));
                    st.expect_seen("stm32f746-nucleo-test",
                              Duration::from_secs(30));
                }
            );
        }
    );
}
