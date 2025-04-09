use crate::device_test::{device_test, DeviceTest};
use crate::ssdp_test::ssdp_test;
use serial_test::*;
use std::panic;
use std::time::Duration;

fn rp2350_test<F: FnOnce(DeviceTest) -> () + panic::UnwindSafe>(
    firmware: &str,
    f: F,
) {
    device_test("RP235x", "COTTON_PROBE_RP2350_W6100", firmware, f);
}

#[test]
#[serial(rp2350_w6100)]
#[cfg_attr(miri, ignore)]
fn arm_rp2350_w6100_0hello() {
    rp2350_test(
        "../cross/rp2350-w6100/target/thumbv8m.main-none-eabihf/debug/rp2350-hello",
        |t| {
            t.expect("rp2350-hello", Duration::from_secs(25));
        },
    );
}
