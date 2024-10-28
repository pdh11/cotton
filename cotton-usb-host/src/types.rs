use crate::debug;
use zerocopy::{Immutable, IntoBytes};

/// A SETUP packet as transmitted on control endpoints.
///
/// All transactions on control endpoints start with a SETUP packet of
/// this format. (Some are then followed by IN or OUT data packets, but
/// others are not).
///
/// The format of this packet (and the un-Rust-like names of its
/// fields) are defined in the USB 2.0 specification, section 9.3.
/// Other sections of the USB specification, and of the specifications
/// of particular device classes, dictate what to put in these fields.
///
/// Control transactions are performed using
/// [`UsbBus::control_transfer()`](crate::usb_bus::UsbBus::control_transfer).
///
/// For instance, here is how to read the MAC address of an AX88772 USB-to-Ethernet adaptor:
///
/// ```no_run
/// # use cotton_usb_host::host_controller::HostController;
/// # use std::pin::{pin, Pin};
/// # use std::task::{Context, Poll, Waker};
/// # use cotton_usb_host::usb_bus::UsbBus;
/// # use cotton_usb_host::host_controller::{InterruptPipe, MultiInterruptPipe, DataPhase, DeviceStatus};
/// # use cotton_usb_host::host_controller::InterruptPacket;
/// # use cotton_usb_host::types::{SetupPacket, UsbError, UsbDevice, DEVICE_TO_HOST, VENDOR_REQUEST, DeviceInfo, UsbSpeed};
/// # use futures::{Stream, StreamExt};
/// # struct Driver;
/// # struct Foo;
/// # impl Stream for Foo {
/// # type Item = DeviceStatus;
/// # fn poll_next(
/// #       mut self: Pin<&mut Self>,
/// #       cx: &mut Context<'_>,
/// #   ) -> Poll<Option<Self::Item>> { todo!() }
/// # }
/// # impl<'a> InterruptPipe for &'a Foo {
/// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
/// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
/// # }
/// # impl InterruptPipe for Foo {
/// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
/// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
/// # }
/// # impl MultiInterruptPipe for Foo {
/// # fn try_add(
/// #  &mut self,
/// # address: u8,
/// # endpoint: u8,
/// #       max_packet_size: u8,
/// #    interval_ms: u8,
/// #   ) -> Result<(), UsbError> { todo!() }
/// # fn remove(&mut self, address: u8) { todo!() }
/// # }
/// # impl HostController for Driver {
/// #     type InterruptPipe<'driver> = &'driver Foo;
/// #     type MultiInterruptPipe = Foo; type DeviceDetect = Foo;
/// # fn device_detect(&self) -> Self::DeviceDetect { todo!() }
/// # fn control_transfer<'a>(&self,
/// #   address: u8,
/// #       packet_size: u8,
/// #       setup: SetupPacket,
/// #       data_phase: DataPhase<'a>,
/// #   ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
/// #  async { todo!() } }
/// # fn alloc_interrupt_pipe(
/// # &self,
/// #  address: u8,
/// #    endpoint: u8,
/// #   max_packet_size: u16,
/// #    interval_ms: u8,
/// # ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>> {
/// # async { todo!() } }
/// #
/// # fn multi_interrupt_pipe(&self) -> Self::MultiInterruptPipe { todo!() }
/// # }
/// # let driver = Driver;
/// # let bus = UsbBus::new(driver);
/// # let device = UsbDevice { address: 1 };
/// # let info = DeviceInfo { vid: 0, pid: 0, class: 0, subclass: 0, speed: UsbSpeed::Low1_5, packet_size_ep0: 8 };
/// # pollster::block_on(async {
/// let mut data = [0u8; 6];
/// let rc = bus.control_transfer(
///         device.address,
///         info.packet_size_ep0,
///         SetupPacket {
///             bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
///             bRequest: 0x13,
///             wValue: 0,
///             wIndex: 0,
///             wLength: 6,
///         },
///         DataPhase::In(&mut data),
///     )
///     .await;
/// # });
/// ```
///
/// Here, the "Request Type" indicates a vendor-specific (AX88772-specific)
/// request, and the "0x13" is taken from the AX88772 datasheet and is the
/// code for "read MAC address". And a MAC address is 6 bytes long, as seen
/// in `wLength`.
///
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-2
pub struct SetupPacket {
    /// The type and specific target of the request.
    pub bmRequestType: u8,
    /// The particular request.
    pub bRequest: u8,
    /// A parameter to the request.
    pub wValue: u16,
    /// A second parameter to the request.
    pub wIndex: u16,
    /// The length of the subsequent IN or OUT data phase; can be zero
    /// if the setup packet itself contains all the required
    /// information.
    pub wLength: u16,
}

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-8
pub struct DeviceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: [u8; 2],
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,

    pub idVendor: [u8; 2],
    pub idProduct: [u8; 2],
    pub bcdDevice: [u8; 2],
    pub iManufacturer: u8,
    pub iProduct: u8,
    pub iSerialNumber: u8,
    pub bNumConfigurations: u8,
}

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Immutable, IntoBytes)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-10
pub struct ConfigurationDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wTotalLength: [u8; 2],
    pub bNumInterfaces: u8,
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,
    pub bmAttributes: u8,
    pub bMaxPower: u8,
}

impl ConfigurationDescriptor {
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= core::mem::size_of::<Self>() {
            Some(unsafe { *(bytes as *const [u8] as *const Self) })
        } else {
            None
        }
    }
}

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Immutable, IntoBytes)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-12
pub struct InterfaceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bNumEndpoints: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,
}

impl InterfaceDescriptor {
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= core::mem::size_of::<Self>() {
            Some(unsafe { *(bytes as *const [u8] as *const Self) })
        } else {
            None
        }
    }
}

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Immutable, IntoBytes)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-13
pub struct EndpointDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: [u8; 2],
    pub bInterval: u8,
}

impl EndpointDescriptor {
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= core::mem::size_of::<Self>() {
            Some(unsafe { *(bytes as *const [u8] as *const Self) })
        } else {
            None
        }
    }
}

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 11-13
pub struct HubDescriptor {
    bDescLength: u8,
    bDescriptorType: u8,
    bNbrPorts: u8,
    wHubCharacteristics: [u8; 2],
    bPwrOn2PwrGood: u8,
    bHubContrCurrent: u8,
    DeviceRemovable: u8, // NB only for hubs up to 8 (true) ports
    PortPwrCtrlMask: u8, // NB only for hubs up to 8 (true) ports
}

impl HubDescriptor {
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= core::mem::size_of::<Self>() {
            Some(unsafe { *(bytes as *const [u8] as *const Self) })
        } else {
            None
        }
    }
}

// For request_type (USB 2.0 table 9-2)
pub const DEVICE_TO_HOST: u8 = 0x80;
pub const HOST_TO_DEVICE: u8 = 0;
pub const STANDARD_REQUEST: u8 = 0;
pub const CLASS_REQUEST: u8 = 0x20;
pub const VENDOR_REQUEST: u8 = 0x40;
pub const RECIPIENT_DEVICE: u8 = 0;
pub const RECIPIENT_INTERFACE: u8 = 1;
pub const RECIPIENT_ENDPOINT: u8 = 2;
pub const RECIPIENT_OTHER: u8 = 3;

// For request (USB 2.0 table 9-4)
pub const GET_STATUS: u8 = 0;
pub const CLEAR_FEATURE: u8 = 1;
pub const SET_FEATURE: u8 = 3;
pub const SET_ADDRESS: u8 = 5;
pub const GET_DESCRIPTOR: u8 = 6;
pub const SET_DESCRIPTOR: u8 = 7;
pub const SET_CONFIGURATION: u8 = 9;

// Descriptor types (USB 2.0 table 9-5)
pub const DEVICE_DESCRIPTOR: u8 = 1;
pub const CONFIGURATION_DESCRIPTOR: u8 = 2;
pub const STRING_DESCRIPTOR: u8 = 3;
pub const INTERFACE_DESCRIPTOR: u8 = 4;
pub const ENDPOINT_DESCRIPTOR: u8 = 5;
pub const HUB_DESCRIPTOR: u8 = 0x29; // USB 2.0 table 11-13

// Class codes (DeviceDescriptor.bDeviceClass)
pub const HUB_CLASSCODE: u8 = 9;

// Values for SET_FEATURE for hubs (USB 2.0 table 11-17)
pub const PORT_RESET: u16 = 4;
pub const PORT_POWER: u16 = 8;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UsbError {
    Nak,
    Stall,
    Timeout,
    Overflow,
    BitStuffError,
    CrcError,
    DataSeqError,
    BufferTooSmall,
    AllPipesInUse,
    ProtocolError,
    TooManyDevices,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UsbSpeed {
    Low1_5,
    Full12,
    High480,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EndpointType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
pub struct DeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub class: u8,
    pub subclass: u8,
    pub speed: UsbSpeed,
    pub packet_size_ep0: u8,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub struct UsbDevice {
    pub address: u8,
}

pub trait DescriptorVisitor {
    fn on_configuration(&mut self, _c: &ConfigurationDescriptor) {}
    fn on_interface(&mut self, _i: &InterfaceDescriptor) {}
    fn on_endpoint(&mut self, _e: &EndpointDescriptor) {}
    fn on_other(&mut self, _d: &[u8]) {}
}

pub struct ShowDescriptors;

impl DescriptorVisitor for ShowDescriptors {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        debug::println!("{:?}", c);
    }
    fn on_interface(&mut self, i: &InterfaceDescriptor) {
        debug::println!("  {:?}", i);
    }
    fn on_endpoint(&mut self, e: &EndpointDescriptor) {
        debug::println!("    {:?}", e);
    }
    fn on_other(&mut self, d: &[u8]) {
        let dlen = d[0];
        let dtype = d[1];
        let domain = match dtype & 0x60 {
            0x00 => "standard",
            0x20 => "class",
            0x40 => "vendor",
            _ => "reserved",
        };
        debug::println!("  {} type {} len {} skipped", domain, dtype, dlen);
    }
}

pub fn parse_descriptors(buf: &[u8], v: &mut impl DescriptorVisitor) {
    let mut index = 0;

    while buf.len() > index + 2 {
        let dlen = buf[index] as usize;
        let dtype = buf[index + 1];

        if dlen < 2 || buf.len() < index + dlen {
            return;
        }

        match dtype {
            CONFIGURATION_DESCRIPTOR => v.on_configuration(
                &ConfigurationDescriptor::try_from_bytes(
                    &buf[index..index + dlen],
                )
                .unwrap(),
            ),
            INTERFACE_DESCRIPTOR => v.on_interface(
                &InterfaceDescriptor::try_from_bytes(
                    &buf[index..index + dlen],
                )
                .unwrap(),
            ),
            ENDPOINT_DESCRIPTOR => v.on_endpoint(
                &EndpointDescriptor::try_from_bytes(&buf[index..index + dlen])
                    .unwrap(),
            ),
            _ => v.on_other(&buf[index..(index + dlen)]),
        }

        index += dlen;
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    extern crate alloc;

    struct Interface {
        descriptor: InterfaceDescriptor,
        endpoints: Vec<EndpointDescriptor>,
    }

    #[derive(Default)]
    struct TestVisitor {
        configuration: Option<ConfigurationDescriptor>,
        interfaces: Vec<Interface>,
    }

    impl DescriptorVisitor for TestVisitor {
        fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
            assert!(self.configuration.is_none());
            self.configuration = Some(*c);
        }

        fn on_interface(&mut self, i: &InterfaceDescriptor) {
            assert!(self.configuration.is_some());
            self.interfaces.push(Interface {
                descriptor: *i,
                endpoints: Vec::new(),
            });
        }

        fn on_endpoint(&mut self, e: &EndpointDescriptor) {
            assert!(!self.interfaces.is_empty());
            self.interfaces.last_mut().unwrap().endpoints.push(*e);
        }

        fn on_other(&mut self, _d: &[u8]) {}
    }

    struct IgnoreVisitor;

    impl DescriptorVisitor for IgnoreVisitor {}

    const ELLA: &[u8] = &[
        9, 2, 180, 1, 5, 1, 0, 128, 250, 9, 4, 0, 0, 4, 255, 0, 3, 0, 12, 95,
        1, 0, 10, 0, 4, 4, 1, 0, 4, 0, 7, 5, 2, 2, 0, 2, 0, 7, 5, 8, 2, 0, 2,
        0, 7, 5, 132, 2, 0, 2, 0, 7, 5, 133, 3, 8, 0, 8, 9, 4, 1, 0, 0, 254,
        1, 1, 0, 9, 33, 1, 200, 0, 0, 4, 1, 1, 16, 64, 8, 8, 11, 1, 1, 3, 69,
        108, 108, 97, 68, 111, 99, 107, 8, 11, 2, 3, 1, 0, 32, 5, 9, 4, 2, 0,
        1, 1, 1, 32, 5, 9, 36, 1, 0, 2, 11, 0, 1, 0, 12, 36, 3, 4, 2, 6, 0,
        14, 11, 4, 0, 0, 8, 36, 10, 10, 1, 7, 0, 0, 8, 36, 10, 11, 1, 7, 0, 0,
        9, 36, 11, 12, 2, 10, 11, 3, 0, 17, 36, 2, 13, 1, 1, 0, 10, 6, 63, 0,
        0, 0, 0, 0, 0, 4, 34, 36, 6, 14, 13, 0, 0, 0, 0, 15, 0, 0, 0, 15, 0,
        0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 0, 64, 36,
        9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 36, 9, 0, 0, 0,
        49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 31, 36, 9, 0, 0, 0, 16, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 5,
        131, 3, 6, 0, 8, 9, 4, 3, 0, 0, 1, 2, 32, 5, 9, 4, 3, 1, 1, 1, 2, 32,
        5, 16, 36, 1, 13, 0, 1, 1, 0, 0, 0, 6, 63, 0, 0, 0, 0, 6, 36, 2, 1, 2,
        16, 7, 5, 9, 13, 64, 2, 4, 8, 37, 1, 0, 0, 1, 0, 0, 9, 4, 4, 0, 0, 1,
        2, 32, 5,
    ];

    const HUB: &[u8] = &[9, 41, 4, 0, 0, 50, 100, 0, 255];

    #[test]
    fn parse_ella() {
        parse_descriptors(ELLA, &mut ShowDescriptors);
        let mut v = TestVisitor::default();
        parse_descriptors(ELLA, &mut v);
        assert!(v.configuration.is_some());
        let cfg = v.configuration.unwrap();
        assert_eq!(cfg.bNumInterfaces, 5);
        assert_eq!(v.interfaces.len(), 6); // one has two AlternateSettings
        assert_eq!(v.interfaces[0].descriptor.bInterfaceClass, 255);
        assert_eq!(v.interfaces[0].endpoints.len(), 4);
        assert_eq!(v.interfaces[0].endpoints[3].bmAttributes, 3);
    }

    #[test]
    fn ignore_ella() {
        parse_descriptors(ELLA, &mut IgnoreVisitor);
    }

    #[test]
    fn configuration_too_small() {
        assert!(ConfigurationDescriptor::try_from_bytes(&[0]).is_none());
    }

    #[test]
    fn interface_too_small() {
        assert!(InterfaceDescriptor::try_from_bytes(&[0]).is_none());
    }

    #[test]
    fn endpoint_too_small() {
        assert!(EndpointDescriptor::try_from_bytes(&[0]).is_none());
    }

    #[test]
    fn hub() {
        let h = HubDescriptor::try_from_bytes(HUB).unwrap();
        assert_eq!(h.bNbrPorts, 4);
        assert_eq!(h.bHubContrCurrent, 100);
    }

    #[test]
    fn hub_too_small() {
        assert!(HubDescriptor::try_from_bytes(&[0]).is_none());
    }

    #[test]
    fn invalid_descriptor() {
        // Mostly a test for Miri
        parse_descriptors(&[9, 41, 1], &mut ShowDescriptors);
    }

    #[test]
    fn reserved_descriptor() {
        // Mostly a test for Miri
        parse_descriptors(&[3, 96, 1], &mut ShowDescriptors);
    }
}
