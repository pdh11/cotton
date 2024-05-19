use crate::device_test::{device_test, DeviceTest};
use crate::ssdp_test::ssdp_test;
use serial_test::*;
use std::panic;
use std::time::Duration;

fn rp2040_test<F: FnOnce(DeviceTest) -> () + panic::UnwindSafe>(
    firmware: &str,
    f: F,
) {
    device_test("RP2040", "COTTON_PROBE_RP2040_W5500", firmware, f);
}

#[test]
#[serial(rp2040_w5500)]
#[cfg_attr(miri, ignore)]
fn arm_rp2040_w5500_hello() {
    rp2040_test(
        "../cross/rp2040-w5500/target/thumbv6m-none-eabi/debug/hello",
        |t| {
            t.expect("Hello RP2040", Duration::from_secs(25));
        },
    );
}

#[test]
#[serial(rp2040_w5500)]
#[cfg_attr(miri, ignore)]
fn arm_rp2040_w5500_dhcp_rtic() {
    rp2040_test(
        "../cross/rp2040-w5500/target/thumbv6m-none-eabi/debug/rp2040-w5500-dhcp-rtic",
        |t| {
            t.expect_stderr("Finished in", Duration::from_secs(45));
            t.expect("DHCP succeeded!", Duration::from_secs(10));
        },
    );
}

#[test]
#[serial(rp2040_w5500)]
#[cfg_attr(miri, ignore)]
fn arm_rp2040_w5500macraw_dhcp_rtic() {
    rp2040_test(
        "../cross/rp2040-w5500/target/thumbv6m-none-eabi/debug/rp2040-w5500macraw-dhcp-rtic",
        |t| {
            t.expect_stderr("Finished in", Duration::from_secs(45));
            t.expect("DHCP config acquired!", Duration::from_secs(10));
        },
    );
}

#[test]
#[serial(rp2040_w5500)]
#[cfg_attr(miri, ignore)]
fn arm_rp2040_w5500macraw_ssdp_rtic() {
    rp2040_test(
        "../cross/rp2040-w5500/target/thumbv6m-none-eabi/debug/rp2040-w5500macraw-ssdp-rtic",
        |nt| {
            nt.expect_stderr("Finished in", Duration::from_secs(45));
            nt.expect("DHCP config acquired!", Duration::from_secs(10));
            ssdp_test(
                Some("cotton-test-server-rp2040".to_string()),
                |st| {
                    nt.expect("SSDP! cotton-test-server-rp2040",
                              Duration::from_secs(20));
                    st.expect_seen("rp2040-w5500-test",
                              Duration::from_secs(10));
                }
            );
        }
    );
}
