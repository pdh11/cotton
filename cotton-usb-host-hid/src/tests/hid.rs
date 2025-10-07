use super::*;
use cotton_usb_host::mocks::{
    MockHostController, MockHostControllerInner, MockInterruptPipe,
};
use cotton_usb_host::usb_bus::{create_test_device, InterruptPacket, UsbBus};
use futures::future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

struct NoOpWaker;

impl Wake for NoOpWaker {
    fn wake(self: Arc<Self>) {}
}

trait PollExtras<T> {
    fn to_option(self) -> Option<T>;
}

impl<T> PollExtras<T> for Poll<T> {
    fn to_option(self) -> Option<T> {
        match self {
            Poll::Ready(t) => Some(t),
            _ => None,
        }
    }
}

struct Fixture<'a> {
    c: &'a mut core::task::Context<'a>,
    h: Hid<'a, MockHostController>,
}

fn do_test<
    SetupFn: FnMut(&mut MockHostControllerInner),
    TestFn: FnMut(Fixture),
>(
    mut setup: SetupFn,
    mut test: TestFn,
) {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();

    setup(&mut hc.inner);
    let bus = UsbBus::new(hc);
    // SAFETY: we don't use this with a non-mock bus
    let device = unsafe { create_test_device(2, 2) };

    let f = Fixture {
        c: &mut c,
        h: Hid::new(&bus, device, 1).unwrap(),
    };

    test(f);
}

#[test]
fn test_new() {
    do_test(|_| {}, |_| {});
}

#[test]
fn test_report_ok() {
    do_test(
        |hc| {
            hc.expect_alloc_interrupt_pipe()
                .withf(|a, _, e, m, i| {
                    *a == 255 && *e == 1 && *m == 8 && *i == 10
                })
                .returning(|_, _, _, _, _| {
                    Box::pin(future::ready({
                        let mut ip = MockInterruptPipe::new();
                        ip.expect_poll_next().returning(|_| {
                            let mut packet = InterruptPacket::new();
                            packet.data[0] = 23;
                            packet.data[1] = 34;
                            Poll::Ready(Some(packet))
                        });
                        ip
                    }))
                });
        },
        |mut f| {
            let h = pin!(f.h.handle());
            let p = h.poll_next(f.c);
            assert!(p.is_ready());
            let packet = p.to_option().unwrap();
            assert!(packet.is_some());
            let packet = packet.unwrap();
            assert_eq!(packet.bytes[0], 23);
            assert_eq!(packet.bytes[1], 34);
        },
    );
}

/// To read raw descriptors:
/// ```
/// $ sudo python3
/// > import usb
/// > dev = usb.core.find(idVendor=0x424, idProduct=0x4064) # as needed
/// > usb.control.get_descriptor(dev, 0x400, usb.util.DESC_TYPE_CONFIG, 0)
/// ```
const DELL_KEYBOARD: &[u8] = &[
    9, 2, 59, 0, 2, 1, 0, 160, 49, 9, 4, 0, 0, 1, 3, 1, 1, 0, 9, 33, 16, 1, 0,
    1, 34, 65, 0, 7, 5, 129, 3, 8, 0, 10, 9, 4, 1, 0, 1, 3, 1, 2, 0, 9, 33,
    16, 1, 0, 1, 34, 216, 0, 7, 5, 130, 3, 8, 0, 10,
];

#[test]
fn test_identify_dell() {
    let mut hid = IdentifyHid::default();
    cotton_usb_host::wire::parse_descriptors(DELL_KEYBOARD, &mut hid);
    assert_eq!(hid.identify(), Some(1));
    assert_eq!(hid.endpoint(), Some(1));
}

const BOSSWARE_THING: &[u8] = &[
    // configuration 1
    9, 2, 59, 0, 2, 1, 4, 160, 49,
    // interface #1: HID mouse
    9, 4, 1, 0, 1, 3, 1, 2, 0,
    // HID dtor
    9, 33, 17, 1, 0, 1, 34, 69, 0,
    // EP dtor: 2, interrupt, IN
    7, 5, 130, 3, 64, 0, 8,
    // interface #3: HID keyboard
    9, 4, 3, 0, 1, 3, 1, 1, 0,
    // HID dtor
    9, 33, 17, 1, 0, 1, 34, 86, 0,
    // EP dtor: 1, interrupt, IN
    7, 5, 129, 3, 64, 0, 8
];

#[test]
fn test_identify_bossware() {
    let mut hid = IdentifyHid::default();
    cotton_usb_host::wire::parse_descriptors(BOSSWARE_THING, &mut hid);
    assert_eq!(hid.identify(), Some(1));
    assert_eq!(hid.endpoint(), Some(1));
}

const GIGABYTE_MOUSE: &[u8] = &[
    9, 2, 34, 0, 1, 1, 0, 160, 50, 9, 4, 0, 0, 1, 3, 1, 2, 0, 9, 33, 17, 1, 0,
    1, 34, 52, 0, 7, 5, 129, 3, 4, 0, 10,
];

#[test]
fn test_dont_identify_hid_mouse() {
    let mut hid = IdentifyHid::default();
    cotton_usb_host::wire::parse_descriptors(GIGABYTE_MOUSE, &mut hid);
    assert_eq!(hid.identify(), None);
}

const HANDBAG: &[u8] = &[
    9, 2, 32, 0, 1, 1, 0, 128, 50, 9, 4, 0, 0, 2, 8, 6, 80, 0, 7, 5, 1, 2, 0,
    2, 0, 7, 5, 129, 2, 0, 2, 0,
];

#[test]
fn test_dont_identify_mass_storage() {
    let mut hid = IdentifyHid::default();
    cotton_usb_host::wire::parse_descriptors(HANDBAG, &mut hid);
    assert_eq!(hid.identify(), None);
}
