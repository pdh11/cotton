use super::*;
use crate::host_controller::tests::{
    MockDeviceDetect, MockHostController, MockHostControllerInner,
    MockInterruptPipe, MockMultiInterruptPipe,
};
use crate::wire::{
    EndpointDescriptor, InterfaceDescriptor, ENDPOINT_DESCRIPTOR,
    INTERFACE_DESCRIPTOR, VENDOR_REQUEST,
};
use futures::{future, Future};
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
extern crate alloc;

struct NoOpWaker;

impl Wake for NoOpWaker {
    fn wake(self: Arc<Self>) {}
}

fn no_delay(_ms: usize) -> impl Future<Output = ()> {
    future::ready(())
}

fn long_delay(_ms: usize) -> impl Future<Output = ()> {
    future::pending()
}

fn short_delay(ms: usize) -> impl Future<Output = ()> {
    if ms > 20 {
        future::Either::Left(future::ready(()))
    } else {
        future::Either::Right(future::pending())
    }
}

const ELLA: &[u8] = &[
    9, 2, 180, 1, 5, 1, 0, 128, 250, 9, 4, 0, 0, 4, 255, 0, 3, 0, 12, 95, 1,
    0, 10, 0, 4, 4, 1, 0, 4, 0, 7, 5, 2, 2, 0, 2, 0, 7, 5, 8, 2, 0, 2, 0, 7,
    5, 132, 2, 0, 2, 0, 7, 5, 133, 3, 8, 0, 8, 9, 4, 1, 0, 0, 254, 1, 1, 0, 9,
    33, 1, 200, 0, 0, 4, 1, 1, 16, 64, 8, 8, 11, 1, 1, 3, 69, 108, 108, 97,
    68, 111, 99, 107, 8, 11, 2, 3, 1, 0, 32, 5, 9, 4, 2, 0, 1, 1, 1, 32, 5, 9,
    36, 1, 0, 2, 11, 0, 1, 0, 12, 36, 3, 4, 2, 6, 0, 14, 11, 4, 0, 0, 8, 36,
    10, 10, 1, 7, 0, 0, 8, 36, 10, 11, 1, 7, 0, 0, 9, 36, 11, 12, 2, 10, 11,
    3, 0, 17, 36, 2, 13, 1, 1, 0, 10, 6, 63, 0, 0, 0, 0, 0, 0, 4, 34, 36, 6,
    14, 13, 0, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0,
    15, 0, 0, 0, 15, 0, 0, 0, 0, 64, 36, 9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    64, 36, 9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 31, 36, 9, 0, 0, 0, 16, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 5,
    131, 3, 6, 0, 8, 9, 4, 3, 0, 0, 1, 2, 32, 5, 9, 4, 3, 1, 1, 1, 2, 32, 5,
    16, 36, 1, 13, 0, 1, 1, 0, 0, 0, 6, 63, 0, 0, 0, 0, 6, 36, 2, 1, 2, 16, 7,
    5, 9, 13, 64, 2, 4, 8, 37, 1, 0, 0, 1, 0, 0, 9, 4, 4, 0, 0, 1, 2, 32, 5,
];

fn example_config_descriptor(buf: &mut [u8]) -> usize {
    let total_length = (core::mem::size_of::<ConfigurationDescriptor>()
        + core::mem::size_of::<InterfaceDescriptor>()
        + core::mem::size_of::<EndpointDescriptor>())
        as u16;

    let c = ConfigurationDescriptor {
        bLength: core::mem::size_of::<ConfigurationDescriptor>() as u8,
        bDescriptorType: CONFIGURATION_DESCRIPTOR,
        wTotalLength: total_length.to_le_bytes(),
        bNumInterfaces: 1,
        bConfigurationValue: 1,
        iConfiguration: 0,
        bmAttributes: 0,
        bMaxPower: 0,
    };

    buf[0..9].copy_from_slice(bytemuck::bytes_of(&c));

    let i = InterfaceDescriptor {
        bLength: core::mem::size_of::<InterfaceDescriptor>() as u8,
        bDescriptorType: INTERFACE_DESCRIPTOR,
        bInterfaceNumber: 1,
        bAlternateSetting: 0,
        bNumEndpoints: 1,
        bInterfaceClass: 0,
        bInterfaceSubClass: 0,
        bInterfaceProtocol: 0,
        iInterface: 0,
    };

    buf[9..18].copy_from_slice(bytemuck::bytes_of(&i));

    let e = EndpointDescriptor {
        bLength: core::mem::size_of::<EndpointDescriptor>() as u8,
        bDescriptorType: ENDPOINT_DESCRIPTOR,
        bEndpointAddress: 1,
        bmAttributes: 0,
        wMaxPacketSize: 64u16.to_le_bytes(),
        bInterval: 0,
    };

    buf[18..25].copy_from_slice(bytemuck::bytes_of(&e));
    25
}

const UNCONFIGURED_DEVICE: UnconfiguredDevice = UnconfiguredDevice {
    usb_address: 5,
    usb_speed: UsbSpeed::Full12,
    packet_size_ep0: 8,
};

fn unconfigured_device() -> UnconfiguredDevice {
    UnconfiguredDevice {
        usb_address: 5,
        usb_speed: UsbSpeed::Full12,
        packet_size_ep0: 8,
    }
}

fn unaddressed_device() -> UnaddressedDevice {
    UnaddressedDevice {
        usb_speed: UsbSpeed::Full12,
        packet_size_ep0: 8,
    }
}

const EXAMPLE_DEVICE: UsbDevice = UsbDevice {
    usb_address: 5,
    usb_speed: UsbSpeed::Full12,
    packet_size_ep0: 8,
};

// Not sure why this isn't in the standard library
fn unwrap_poll<T>(p: Poll<T>) -> Option<T> {
    match p {
        Poll::Ready(t) => Some(t),
        _ => None,
    }
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

#[test]
fn unwrap_good_poll() {
    let p = Poll::Ready(1);
    assert!(unwrap_poll(p).is_some());
}

#[test]
fn unwrap_bad_poll() {
    let p = Poll::<u32>::Pending;
    assert!(unwrap_poll(p).is_none());
}

#[test]
fn basic_configuration() {
    let mut bc = BasicConfiguration::default();
    crate::wire::parse_descriptors(ELLA, &mut bc);

    assert_eq!(bc.configuration_value, 1);
    assert_eq!(bc.num_configurations, 1);
    assert_eq!(bc.in_endpoints, 0b111000);
    assert_eq!(bc.out_endpoints, 0b1100000100);
}

#[test]
fn new_bus() {
    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);
    let _bus = UsbBus::new(hc);
}

fn is_set_configuration<const ADDR: u8, const N: u16>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == ADDR
        && *p == 8
        && s.bmRequestType == HOST_TO_DEVICE
        && s.bRequest == SET_CONFIGURATION
        && s.wValue == N
        && s.wIndex == 0
        && s.wLength == 0
        && d.is_none()
}

fn control_transfer_ok<const N: usize>(
    _: u8,
    _: u8,
    _: SetupPacket,
    _: DataPhase,
) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
    Box::pin(future::ready(Ok(N)))
}

// This is by some margin the most insane function signature I have yet
// written in Rust -- but it does make its call sites neater!
#[rustfmt::skip]
fn control_transfer_ok_with<F: FnMut(&mut [u8]) -> usize>(
    mut f: F,
) -> impl FnMut(
    u8,
    u8,
    SetupPacket,
    DataPhase,
) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
    move |_, _, _, mut d| {
        let mut n = 0;
        d.in_with(|bytes| n = f(bytes));
        Box::pin(future::ready(Ok(n)))
    }
}

fn control_transfer_pending(
    _: u8,
    _: u8,
    _: SetupPacket,
    _: DataPhase,
) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
    Box::pin(future::pending())
}

fn control_transfer_timeout(
    _: u8,
    _: u8,
    _: SetupPacket,
    _: DataPhase,
) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
    Box::pin(future::ready(Err(UsbError::Timeout)))
}

trait ExtraExpectations {
    fn expect_multi_interrupt_pipe_ignored(&mut self);
    fn expect_add_to_multi_interrupt_pipe(&mut self);

    /// Expect a call to get_basic_configuration (for a certain address),
    /// which reads the configuration descriptor.
    fn expect_get_configuration<const ADDR: u8>(&mut self);

    /// Expect a call to configure (for a certain address and
    /// configuration number) which does a control transfer.
    fn expect_set_configuration<const ADDR: u8, const VALUE: u16>(&mut self);

    /// Expect a control transfer to read the hub descriptor from a certain
    /// address.
    fn expect_get_hub_descriptor<const ADDR: u8>(&mut self);

    /// Expect a control transfer (for a certain address and port
    /// number) to set a port power feature.
    fn expect_set_port_power<const ADDR: u8, const PORT: u8>(&mut self);

    /// Expect a get-port-status command for a specific port, returning a
    /// specific state and changeset.
    fn expect_get_port_status<
        const PORT: u8,
        const STATE: u16,
        const CHANGES: u16,
    >(
        &mut self,
    );

    /// Expect a set-port-feature command for a specific port, enabling a
    /// specific feature.
    fn expect_set_port_feature<const PORT: u8, const FEATURE: u16>(&mut self);

    /// Expect a clear-port-feature command for a specific port, clearing a
    /// specific feature.
    fn expect_clear_port_feature<const PORT: u8, const FEATURE: u16>(
        &mut self,
    );

    fn expect_get_device_descriptor_prefix(&mut self);
    fn expect_get_device_descriptor(&mut self);
    fn expect_set_address<const ADDR: u8>(&mut self);
    fn expect_get_device_descriptor_prefix_hub(&mut self);
    fn expect_get_device_descriptor_hub(&mut self);
}

impl ExtraExpectations for MockHostControllerInner {
    fn expect_add_to_multi_interrupt_pipe(&mut self) {
        self.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });
    }

    fn expect_multi_interrupt_pipe_ignored(&mut self) {
        self.expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
    }

    fn expect_get_configuration<const ADDR: u8>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<ADDR>)
            .returning(control_transfer_ok_with(example_config_descriptor));
    }

    fn expect_set_configuration<const ADDR: u8, const VALUE: u16>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<ADDR, VALUE>)
            .returning(control_transfer_ok::<0>);
    }

    fn expect_get_hub_descriptor<const ADDR: u8>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<ADDR>)
            .returning(control_transfer_ok_with(hub_descriptor));
    }

    fn expect_set_port_power<const ADDR: u8, const PORT: u8>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<ADDR, PORT>)
            .returning(control_transfer_ok::<0>);
    }

    fn expect_get_port_status<
        const PORT: u8,
        const STATE: u16,
        const CHANGES: u16,
    >(
        &mut self,
    ) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<PORT>)
            .returning(control_transfer_ok_with(
                port_status::<STATE, CHANGES>,
            ));
    }

    fn expect_set_port_feature<const PORT: u8, const FEATURE: u16>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_set_port_feature::<PORT, FEATURE>)
            .returning(control_transfer_ok::<0>);
    }

    fn expect_clear_port_feature<const PORT: u8, const FEATURE: u16>(
        &mut self,
    ) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<PORT, FEATURE>)
            .returning(control_transfer_ok::<0>);
    }
    fn expect_get_device_descriptor_prefix(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));
    }
    fn expect_get_device_descriptor(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));
    }
    fn expect_set_address<const ADDR: u8>(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_set_address::<ADDR>)
            .returning(control_transfer_ok::<0>);
    }
    fn expect_get_device_descriptor_prefix_hub(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));
    }
    fn expect_get_device_descriptor_hub(&mut self) {
        self.expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));
    }
}

struct Fixture<'a> {
    c: &'a mut core::task::Context<'a>,
    hub_state: HubState<MockHostController>,
    bus: UsbBus<MockHostController>,
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

    let f = Fixture {
        c: &mut c,
        hub_state: HubState::new(&hc),
        bus: UsbBus::new(hc),
    };

    test(f);
}

#[test]
fn configure() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_set_configuration::<5, 6>();
        },
        |f| {
            let r = pin!(f.bus.configure(unconfigured_device(), 6));
            let rr = r.poll(f.c).to_option().unwrap();
            assert_eq!(rr, Ok(EXAMPLE_DEVICE));
        },
    );
}

#[test]
fn configure_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_configuration::<5, 6>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut r = pin!(f.bus.configure(unconfigured_device(), 6));
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
        },
    );
}

#[test]
fn configure_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_configuration::<5, 6>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let r = pin!(f.bus.configure(unconfigured_device(), 6));
            let rr = r.poll(f.c);
            assert_eq!(rr, Poll::Ready(Err(UsbError::Timeout)));
        },
    );
}

fn is_get_configuration_descriptor<const ADDR: u8>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == ADDR
        && *p == 8
        && s.bmRequestType == DEVICE_TO_HOST
        && s.bRequest == GET_DESCRIPTOR
        && s.wValue == 0x200
        && s.wIndex == 0
        && s.wLength > 0
        && d.is_in()
}

#[test]
fn get_basic_configuration() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_configuration::<5>();
        },
        |f| {
            let r = pin!(f.bus.get_basic_configuration(&UNCONFIGURED_DEVICE));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert!(rc.is_ok());
        },
    );
}

#[test]
fn get_basic_configuration_bad_descriptors() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_configuration_descriptor::<5>)
        .returning(control_transfer_ok::<25>);

    let bus = UsbBus::new(hc);

    let r = pin!(bus.get_basic_configuration(&UNCONFIGURED_DEVICE));
    let rr = r.poll(&mut c);
    assert_eq!(rr, Poll::Ready(Err(UsbError::ProtocolError)));
}

#[test]
fn get_basic_configuration_bad_configuration_value() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_configuration_descriptor::<5>)
        .returning(control_transfer_ok_with(|bytes| {
            example_config_descriptor(bytes);
            bytes[5] = 0; // nobble bConfigurationValue
            25
        }));

    let bus = UsbBus::new(hc);

    let r = pin!(bus.get_basic_configuration(&UNCONFIGURED_DEVICE));
    let rr = r.poll(&mut c);
    assert_eq!(rr, Poll::Ready(Err(UsbError::ProtocolError)));
}

#[test]
fn get_basic_configuration_pends() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_configuration_descriptor::<5>)
        .returning(control_transfer_pending);

    let bus = UsbBus::new(hc);

    let mut r = pin!(bus.get_basic_configuration(&UNCONFIGURED_DEVICE));
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
}

#[test]
fn get_basic_configuration_fails() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_configuration_descriptor::<5>)
        .returning(control_transfer_timeout);

    let bus = UsbBus::new(hc);

    let mut r = pin!(bus.get_basic_configuration(&UNCONFIGURED_DEVICE));
    let rr = r.as_mut().poll(&mut c);
    assert_eq!(rr, Poll::Ready(Err(UsbError::Timeout)));
}

fn is_set_address<const N: u8>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 0
        && *p == 8
        && s.bmRequestType == HOST_TO_DEVICE
        && s.bRequest == SET_ADDRESS
        && s.wValue == N as u16
        && s.wIndex == 0
        && s.wLength == 0
        && d.is_none()
}

#[test]
fn set_address() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_set_address::<5>)
        .returning(control_transfer_ok::<0>);

    let bus = UsbBus::new(hc);

    let r = pin!(bus.set_address(unaddressed_device(), 5));
    let rr = r.poll(&mut c);
    assert!(rr == Poll::Ready(Ok(unconfigured_device())));
}

#[test]
fn set_address_pends() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_set_address::<5>)
        .returning(control_transfer_pending);

    let bus = UsbBus::new(hc);

    let mut r = pin!(bus.set_address(unaddressed_device(), 5));
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
}

#[test]
fn set_address_fails() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_set_address::<5>)
        .returning(control_transfer_timeout);

    let bus = UsbBus::new(hc);

    let r = pin!(bus.set_address(unaddressed_device(), 5));
    let rr = r.poll(&mut c);
    assert!(rr.is_ready());
    assert!(rr == Poll::Ready(Err(UsbError::Timeout)));
}

#[test]
fn interrupt_endpoint_in() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);
    hc.inner
        .expect_alloc_interrupt_pipe()
        .withf(|a, e, m, i| *a == 5 && *e == 2 && *m == 8 && *i == 10)
        .returning(|_, _, _, _| {
            Box::pin(future::ready({
                let mut ip = MockInterruptPipe::new();
                ip.expect_set_waker().return_const(());
                ip.expect_poll()
                    .returning(|| Some(InterruptPacket::default()));
                ip
            }))
        });
    let bus = UsbBus::new(hc);

    let r = pin!(bus.interrupt_endpoint_in(5, 2, 8, 10));
    let rr = r.poll_next(&mut c);
    assert!(rr.is_ready());
}

#[test]
fn interrupt_endpoint_in_pends() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);
    hc.inner
        .expect_alloc_interrupt_pipe()
        .withf(|a, e, m, i| *a == 5 && *e == 2 && *m == 8 && *i == 10)
        .returning(|_, _, _, _| Box::pin(future::pending()));
    let bus = UsbBus::new(hc);

    let mut r = pin!(bus.interrupt_endpoint_in(5, 2, 8, 10));
    let rr = r.as_mut().poll_next(&mut c);
    assert!(rr.is_pending());
    let rr = r.as_mut().poll_next(&mut c);
    assert!(rr.is_pending());
}

fn is_get_device_descriptor<const N: u16>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 0
        && *p == 8
        && s.bmRequestType == DEVICE_TO_HOST
        && s.bRequest == GET_DESCRIPTOR
        && s.wValue == 0x100
        && s.wIndex == 0
        && s.wLength == N
        && d.is_in()
}

fn device_descriptor_prefix(bytes: &mut [u8]) -> usize {
    bytes[0] = 18;
    bytes[1] = DEVICE_DESCRIPTOR;
    bytes[7] = 8;
    8
}

fn device_descriptor(bytes: &mut [u8]) -> usize {
    device_descriptor_prefix(bytes);
    bytes[8] = 0x34;
    bytes[9] = 0x12;
    bytes[10] = 0x78;
    bytes[11] = 0x56;
    18
}

#[test]
fn new_device() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_ok_with(device_descriptor_prefix));

    // Second call (wLength == 18)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<18>)
        .returning(control_transfer_ok_with(device_descriptor));

    let bus = UsbBus::new(hc);

    let r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.poll(&mut c);
    let (_device, di) = unwrap_poll(rr).unwrap().unwrap();
    assert_eq!(di.vid, 0x1234);
    assert_eq!(di.pid, 0x5678);
}

#[test]
fn new_device_first_call_errors() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_timeout);

    // No second call!

    let bus = UsbBus::new(hc);

    let r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.poll(&mut c);
    let rc = unwrap_poll(rr).unwrap();
    assert_eq!(rc.unwrap_err(), UsbError::Timeout);
}

#[test]
fn new_device_first_call_short() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_ok::<7>);

    // No second call!

    let bus = UsbBus::new(hc);

    let r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.poll(&mut c);
    let rc = unwrap_poll(rr).unwrap();
    assert_eq!(rc.unwrap_err(), UsbError::ProtocolError);
}

#[test]
fn new_device_second_call_errors() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_ok_with(device_descriptor_prefix));

    // Second call (wLength == 18)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<18>)
        .returning(control_transfer_timeout);

    let bus = UsbBus::new(hc);

    let r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.poll(&mut c);
    let rc = unwrap_poll(rr).unwrap();
    assert_eq!(rc.unwrap_err(), UsbError::Timeout);
}

#[test]
fn new_device_second_call_pends() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_ok_with(device_descriptor_prefix));

    // Second call (wLength == 18)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<18>)
        .returning(control_transfer_pending);

    let bus = UsbBus::new(hc);

    let mut r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
    let rr = r.as_mut().poll(&mut c);
    assert!(rr.is_pending());
}

#[test]
fn new_device_second_call_short() {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();
    hc.inner
        .expect_multi_interrupt_pipe()
        .returning(MockMultiInterruptPipe::new);

    // First call (wLength == 8)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<8>)
        .returning(control_transfer_ok_with(device_descriptor_prefix));

    // Second call (wLength == 18)
    hc.inner
        .expect_control_transfer()
        .times(1)
        .withf(is_get_device_descriptor::<18>)
        .returning(control_transfer_ok::<17>);

    let bus = UsbBus::new(hc);

    let r = pin!(bus.new_device(UsbSpeed::Full12));
    let rr = r.poll(&mut c);
    let rc = unwrap_poll(rr).unwrap();
    assert_eq!(rc.unwrap_err(), UsbError::ProtocolError);
}

fn is_get_hub_descriptor<const ADDR: u8>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == ADDR
        && *p == 8
        && s.bmRequestType == DEVICE_TO_HOST | CLASS_REQUEST
        && s.bRequest == GET_DESCRIPTOR
        && s.wValue == 0x2900
        && s.wIndex == 0
        && s.wLength >= 9
        && d.is_in()
}

fn hub_descriptor(bytes: &mut [u8]) -> usize {
    bytes[0] = 9;
    bytes[1] = HUB_DESCRIPTOR;
    bytes[2] = 2; // 2-port hub
    9
}

fn giant_hub_descriptor(bytes: &mut [u8]) -> usize {
    bytes[0] = 9;
    bytes[1] = HUB_DESCRIPTOR;
    bytes[2] = 15; // 15-port hub
    11 // NB bigger than normal
}

fn is_set_port_power<const ADDR: u8, const N: u8>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == ADDR
        && *p == 8
        && s.bmRequestType == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
        && s.bRequest == SET_FEATURE
        && s.wValue == PORT_POWER
        && s.wIndex == N.into()
        && s.wLength == 0
        && d.is_none()
}

#[test]
fn new_hub() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();
            hc.expect_get_hub_descriptor::<5>();
            hc.expect_set_port_power::<5, 1>();
            hc.expect_set_port_power::<5, 2>();
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert!(rc.is_ok());
        },
    );
}

#[test]
fn new_hub_giant() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();

            // Get hub descriptor
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_hub_descriptor::<5>)
                .returning(control_transfer_ok_with(giant_hub_descriptor));

            // Set port power x15
            hc.expect_control_transfer()
                .times(15)
                .returning(control_transfer_ok::<0>);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert!(rc.is_ok());
        },
    );
}

#[test]
fn new_hub_get_configuration_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();

            // Call to get_basic_configuration
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_configuration_descriptor::<5>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn new_hub_configure_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_configuration::<5>();

            // Call to configure
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_configuration::<5, 1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn new_hub_configure_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_configuration::<5>();

            // Call to configure
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_configuration::<5, 1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut r =
                pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
        },
    );
}

#[test]
fn new_hub_try_add_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe().returning(|| {
                let mut mip = MockMultiInterruptPipe::new();
                mip.expect_try_add()
                    .returning(|_, _, _, _| Err(UsbError::TooManyDevices));
                mip
            });
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::TooManyDevices));
        },
    );
}

#[test]
fn new_hub_get_descriptor_fails() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();

            // Get hub descriptor
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_hub_descriptor::<5>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn new_hub_get_descriptor_short() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();

            // Get hub descriptor
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_hub_descriptor::<5>)
                .returning(control_transfer_ok::<8>);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::ProtocolError));
        },
    );
}

#[test]
fn new_hub_get_descriptor_pends() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();

            // Get hub descriptor
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_hub_descriptor::<5>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut r =
                pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
        },
    );
}

#[test]
fn new_hub_set_port_power_fails() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();
            hc.expect_get_hub_descriptor::<5>();

            // Set port power
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_port_power::<5, 1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let r = pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.poll(f.c);
            let rc = unwrap_poll(rr).unwrap();
            assert_eq!(rc, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn new_hub_set_port_power_pends() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_configuration::<5>();
            hc.expect_set_configuration::<5, 1>();
            hc.expect_get_hub_descriptor::<5>();

            // Set port power
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_port_power::<5, 1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut r =
                pin!(f.bus.new_hub(&f.hub_state, unconfigured_device()));
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
            let rr = r.as_mut().poll(f.c);
            assert_eq!(rr, Poll::Pending);
        },
    );
}

#[test]
fn handle_hub_packet_empty() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.size = 1;
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Ok(DeviceEvent::None));
        },
    );
}

fn is_get_port_status<const N: u8>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 5
        && *p == 8
        && s.bmRequestType == DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER
        && s.bRequest == GET_STATUS
        && s.wValue == 0
        && s.wIndex == N as u16
        && s.wLength == 4
        && d.is_in()
}

fn port_status<const STATE: u16, const CHANGES: u16>(
    bytes: &mut [u8],
) -> usize {
    bytes[0..2].copy_from_slice(&STATE.to_le_bytes());
    bytes[2..4].copy_from_slice(&CHANGES.to_le_bytes());
    4
}

fn is_clear_port_feature<const PORT: u8, const FEATURE: u16>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 5
        && *p == 8
        && s.bmRequestType == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
        && s.bRequest == 1
        && s.wValue == FEATURE
        && s.wIndex == PORT as u16
        && s.wLength == 0
        && d.is_none()
}

fn is_set_port_feature<const PORT: u8, const FEATURE: u16>(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 5
        && *p == 8
        && s.bmRequestType == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
        && s.bRequest == 3
        && s.wValue == FEATURE
        && s.wIndex == PORT as u16
        && s.wLength == 0
        && d.is_none()
}

#[test]
fn handle_hub_packet_connection() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            hc.expect_set_address::<31>();
            // The new device is NOT a hub so we're now done
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));
            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Ok(DeviceEvent::Connect(
                    UnconfiguredDevice {
                        usb_address: 31,
                        usb_speed: UsbSpeed::Full12,
                        packet_size_ep0: 8
                    },
                    DeviceInfo {
                        vid: 0x1234,
                        pid: 0x5678,
                        class: 0,
                        subclass: 0
                    }
                ))
            );
        },
    );
}

#[test]
fn handle_hub_packet_no_changes() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<8, 0, 0>();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 2;
            p.data[0] = 0;
            p.data[1] = 1; // bit 8 set => port 8 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Ok(DeviceEvent::None));
        },
    );
}

#[test]
fn handle_hub_packet_crazy_changes() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<8, 0, 0x20>();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 2;
            p.data[0] = 0;
            p.data[1] = 1; // bit 8 set => port 8 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));
            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Ok(DeviceEvent::None));
        },
    );
}

#[test]
fn handle_hub_packet_connection_status_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();

            // Get port status
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_port_status::<1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connection_status_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();

            // Get port status
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_port_status::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connection_clear_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
                                                    // Clear C_PORT_CONNECTION
            hc.expect_control_transfer()
                .times(1)
                .withf(is_clear_port_feature::<1, 16>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connection_clear_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
                                                    // Clear C_PORT_CONNECTION
            hc.expect_control_transfer()
                .times(1)
                .withf(is_clear_port_feature::<1, 16>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connection_set_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION

            // Set PORT_RESET
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_port_feature::<1, 4>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connection_set_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION

            // Set PORT_RESET
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_port_feature::<1, 4>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connection_second_status_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET

            // Get port status
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_port_status::<1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connection_delay_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, long_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connection_second_status_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET

            // Get port status
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_port_status::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connection_second_status_not_connected() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 0, 0>(); // none
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));
            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Ok(DeviceEvent::None));
        },
    );
}

#[test]
fn handle_hub_packet_disconnection() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 0, 1>(); // C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
        },
        |f| {
            {
                // Set up topology so there's a device (31) on hub 5 port 1
                let mut b = f.hub_state.topology.borrow_mut();
                b.device_connect(0, 1, true); // 1
                b.device_connect(1, 1, true); // 2
                b.device_connect(1, 2, true); // 3
                b.device_connect(1, 3, true); // 4
                b.device_connect(1, 4, true); // 5
                b.device_connect(5, 1, false); // 31
            }

            assert_eq!(
                format!("{:?}", f.hub_state.topology()),
                "0:(1:(2 3 4 5:(31)))"
            );

            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Ok(DeviceEvent::Disconnect(BitSet(0x8000_0000)))
            );
        },
    );
}

// A bit unlikely as we only have FS hardware, but the protocol
// allows for it
#[test]
fn handle_hub_packet_connected_high_speed() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 0x411, 1>(); // high-speed!
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 0x413, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            hc.expect_set_address::<31>();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Ok(DeviceEvent::Connect(
                    UnconfiguredDevice {
                        usb_address: 31,
                        usb_speed: UsbSpeed::High480,
                        packet_size_ep0: 8
                    },
                    DeviceInfo {
                        vid: 0x1234,
                        pid: 0x5678,
                        class: 0,
                        subclass: 0,
                    }
                ))
            );
        },
    );
}

#[test]
fn handle_hub_packet_connected_low_speed() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 0x211, 1>(); // low-speed!
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 0x213, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            hc.expect_set_address::<31>();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Ok(DeviceEvent::Connect(
                    UnconfiguredDevice {
                        usb_address: 31,
                        usb_speed: UsbSpeed::Low1_5,
                        packet_size_ep0: 8
                    },
                    DeviceInfo {
                        vid: 0x1234,
                        pid: 0x5678,
                        class: 0,
                        subclass: 0,
                    }
                ))
            );
        },
    );
}

#[test]
fn handle_hub_packet_enabled_port_reset_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 0x11, 0x10>();

            // Clear C_PORT_RESET
            hc.expect_control_transfer()
                .times(1)
                .withf(is_clear_port_feature::<1, 20>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_enabled_port_reset_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 0x11, 0x10>();

            // Clear C_PORT_RESET
            hc.expect_control_transfer()
                .times(1)
                .withf(is_clear_port_feature::<1, 20>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_connected_new_device_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connected_new_device_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_enabled_set_address_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();

            // Set address (31)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<31>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connected_set_address_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();

            // Set address (31)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<31>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

fn device_descriptor_prefix_hub(bytes: &mut [u8]) -> usize {
    bytes[0] = 18;
    bytes[1] = DEVICE_DESCRIPTOR;
    bytes[4] = HUB_CLASSCODE;
    bytes[7] = 8;
    8
}

fn device_descriptor_hub(bytes: &mut [u8]) -> usize {
    device_descriptor_prefix(bytes);
    bytes[8] = 0x34;
    bytes[9] = 0x12;
    bytes[10] = 0x78;
    bytes[11] = 0x56;
    18
}

#[test]
fn handle_hub_packet_connected_hub() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();

            // new_hub()
            hc.expect_get_configuration::<1>();
            hc.expect_set_configuration::<1, 1>();
            hc.expect_get_hub_descriptor::<1>();
            hc.expect_set_port_power::<1, 1>();
            hc.expect_set_port_power::<1, 2>();
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Ok(DeviceEvent::HubConnect(UsbDevice {
                    usb_address: 1,
                    usb_speed: UsbSpeed::Full12,
                    packet_size_ep0: 8
                },))
            );
        },
    );
}

#[test]
fn handle_hub_packet_connected_hub_new_hub_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();

            // new_hub(): get_basic_configuration
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_configuration_descriptor::<1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::Timeout));
        },
    );
}

#[test]
fn handle_hub_packet_connected_hub_new_hub_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();

            // new_hub(): get_basic_configuration
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_configuration_descriptor::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let mut fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn handle_hub_packet_enabled_too_many_devices() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_get_port_status::<1, 1, 1>(); // CONNECTION, C_PORT_CONNECTION
            hc.expect_clear_port_feature::<1, 16>(); // C_PORT_CONNECTION
            hc.expect_set_port_feature::<1, 4>(); // PORT_RESET
            hc.expect_get_port_status::<1, 3, 0>(); // ENABLED
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
        },
        |f| {
            {
                let mut state = f.hub_state.topology.borrow_mut();
                for i in 1..16 {
                    state.device_connect(0, i, true);
                }
            }

            let mut p = InterruptPacket::new();
            p.address = 5;
            p.size = 1;
            p.data[0] = 0b10; // bit 1 set => port 1 needs attention
            let fut =
                pin!(f.bus.handle_hub_packet(&f.hub_state, &p, no_delay));
            let poll = fut.poll(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Err(UsbError::TooManyDevices));
        },
    );
}

#[test]
fn device_events_nh() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            hc.expect_set_address::<1>();
        },
        |f| {
            let stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::Connect(
                    UnconfiguredDevice {
                        usb_address: 1,
                        usb_speed: UsbSpeed::Full12,
                        packet_size_ep0: 8
                    },
                    DeviceInfo {
                        vid: 0x1234,
                        pid: 0x5678,
                        class: 0,
                        subclass: 0,
                    }
                ))
            );
        },
    );
}

#[test]
fn device_events_nh_first_delay_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
        },
        |f| {
            let mut stream = pin!(f.bus.device_events_no_hubs(long_delay));

            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_nh_second_delay_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
        },
        |f| {
            let mut stream = pin!(f.bus.device_events_no_hubs(short_delay));

            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_nh_new_device_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
            );
        },
    );
}

#[test]
fn device_events_nh_new_device_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_nh_set_address_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            // Set address (1)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
            );
        },
    );
}

#[test]
fn device_events_nh_set_address_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();

            // Set address (1)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_nh_disconnect() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next()
                    .returning(|_| Poll::Ready(Some(DeviceStatus::Absent)));
                mdd
            });
        },
        |f| {
            let stream = pin!(f.bus.device_events_no_hubs(no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF)))
            );
        },
    );
}

#[test]
fn device_events_root_connect() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
                });
                mdd
            });

            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();
            hc.expect_set_address::<31>();
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::Connect(
                    UnconfiguredDevice {
                        usb_address: 31,
                        usb_speed: UsbSpeed::Low1_5,
                        packet_size_ep0: 8
                    },
                    DeviceInfo {
                        vid: 0x1234,
                        pid: 0x5678,
                        class: 0,
                        subclass: 0,
                    }
                ))
            );
        },
    );
}

#[test]
fn device_events_first_delay_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
                });
                mdd
            });

            hc.expect_reset_root_port().withf(|r| *r).return_const(());
        },
        |f| {
            let mut stream =
                pin!(f.bus.device_events(&f.hub_state, long_delay));

            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_second_delay_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });

            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
        },
        |f| {
            let mut stream =
                pin!(f.bus.device_events(&f.hub_state, short_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_new_device_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
            );
        },
    );
}

#[test]
fn device_events_new_device_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());

            // new_device(): first call (wLength == 8)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_device_descriptor::<8>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_set_address_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();

            // Set address (31)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<31>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
            );
        },
    );
}

#[test]
fn device_events_set_address_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix();
            hc.expect_get_device_descriptor();

            // Set address (31)
            hc.expect_control_transfer()
                .times(1)
                .withf(is_set_address::<31>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_root_disconnect() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next()
                    .returning(|_| Poll::Ready(Some(DeviceStatus::Absent)));
                mdd
            });
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF)))
            );
        },
    );
}

#[test]
fn device_events_root_connect_is_hub() {
    do_test(
        |hc| {
            hc.expect_add_to_multi_interrupt_pipe();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();
            hc.expect_get_configuration::<1>();
            hc.expect_set_configuration::<1, 1>();
            hc.expect_get_hub_descriptor::<1>();
            hc.expect_set_port_power::<1, 1>();
            hc.expect_set_port_power::<1, 2>();
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::HubConnect(UsbDevice {
                    usb_address: 1,
                    usb_speed: UsbSpeed::Low1_5,
                    packet_size_ep0: 8
                },))
            );
        },
    );
}

#[test]
fn device_events_root_connect_new_hub_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();

            // Call to get_basic_configuration
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_configuration_descriptor::<1>)
                .returning(control_transfer_timeout);
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
            );
        },
    );
}

#[test]
fn device_events_root_connect_new_hub_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe_ignored();
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| {
                    Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
                });
                mdd
            });
            hc.expect_reset_root_port().withf(|r| *r).return_const(());
            hc.expect_reset_root_port().withf(|r| !*r).return_const(());
            hc.expect_get_device_descriptor_prefix_hub();
            hc.expect_get_device_descriptor_hub();
            hc.expect_set_address::<1>();

            // Call to get_basic_configuration
            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_configuration_descriptor::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

#[test]
fn device_events_hub_packet() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe().returning(|| {
                let mut mip = MockMultiInterruptPipe::new();
                mip.expect_set_waker().return_const(());
                mip.expect_poll().returning(|| {
                    let mut ip = InterruptPacket::new();
                    ip.size = 1;
                    Some(ip)
                });
                mip
            });
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| Poll::Pending);
                mdd
            });
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));

            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(result, Some(DeviceEvent::None));
        },
    );
}

#[test]
fn device_events_hub_packet_fails() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe().returning(|| {
                let mut mip = MockMultiInterruptPipe::new();
                mip.expect_set_waker().return_const(());
                mip.expect_poll().returning(|| {
                    Some(InterruptPacket::new()) // a 0-length packet
                });
                mip
            });
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| Poll::Pending);
                mdd
            });
        },
        |f| {
            let stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.poll_next(f.c);
            let result = unwrap_poll(poll).unwrap();
            assert_eq!(
                result,
                Some(DeviceEvent::EnumerationError(
                    0,
                    1,
                    UsbError::ProtocolError
                ))
            );
        },
    );
}

#[test]
fn device_events_hub_packet_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe().returning(|| {
                let mut mip = MockMultiInterruptPipe::new();
                mip.expect_set_waker().return_const(());
                mip.expect_poll().returning(|| {
                    let mut ip = InterruptPacket::new();
                    ip.size = 1;
                    ip.address = 5;
                    ip.data[0] = 2;
                    Some(ip)
                });
                mip
            });
            hc.expect_device_detect().returning(|| {
                let mut mdd = MockDeviceDetect::new();
                mdd.expect_poll_next().returning(|_| Poll::Pending);
                mdd
            });

            hc.expect_control_transfer()
                .times(1)
                .withf(is_get_port_status::<1>)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut stream = pin!(f.bus.device_events(&f.hub_state, no_delay));
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
            let poll = stream.as_mut().poll_next(f.c);
            assert!(poll.is_pending());
        },
    );
}

fn is_read_mac_address(
    a: &u8,
    p: &u8,
    s: &SetupPacket,
    d: &DataPhase,
) -> bool {
    *a == 5
        && *p == 8
        && s.bmRequestType == DEVICE_TO_HOST | VENDOR_REQUEST
        && s.bRequest == 0x13
        && s.wValue == 0
        && s.wIndex == 0
        && s.wLength == 6
        && d.is_in()
}

#[test]
fn control_transfer() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe()
                .returning(MockMultiInterruptPipe::new);
            hc.expect_control_transfer()
                .times(1)
                .withf(is_read_mac_address)
                .returning(control_transfer_ok_with(|b| {
                    b[0] = 1;
                    6
                }));
        },
        |f| {
            let mut data = [0u8; 6];
            let fut = pin!(f.bus.control_transfer(
                &EXAMPLE_DEVICE,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
                    bRequest: 0x13,
                    wValue: 0,
                    wIndex: 0,
                    wLength: 6,
                },
                DataPhase::In(&mut data),
            ));

            let poll = fut.poll(f.c);
            assert!(poll.is_ready());
        },
    );
}

#[test]
fn control_transfer_pends() {
    do_test(
        |hc| {
            hc.expect_multi_interrupt_pipe()
                .returning(MockMultiInterruptPipe::new);
            hc.expect_control_transfer()
                .times(1)
                .withf(is_read_mac_address)
                .returning(control_transfer_pending);
        },
        |f| {
            let mut data = [0u8; 6];
            let mut fut = pin!(f.bus.control_transfer(
                &EXAMPLE_DEVICE,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
                    bRequest: 0x13,
                    wValue: 0,
                    wIndex: 0,
                    wLength: 6,
                },
                DataPhase::In(&mut data),
            ));

            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
            let poll = fut.as_mut().poll(f.c);
            assert!(poll.is_pending());
        },
    );
}
