use crate::debug;

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
/// For instance, here is how to read the MAC address of an AX88772
/// USB-to-Ethernet adaptor:
///
/// ```no_run
/// # use cotton_usb_host::host_controller::{UsbError, HostController, DataPhase};
/// # use cotton_usb_host::usb_bus::{UsbBus, UsbDevice, DeviceInfo};
/// # use cotton_usb_host::wire::{SetupPacket, DEVICE_TO_HOST, VENDOR_REQUEST};
/// # use futures::{Stream, StreamExt};
/// # async fn foo<HC: HostController>(bus: UsbBus<HC>, device: UsbDevice, info: DeviceInfo) {
/// let mut data = [0u8; 6];
/// let rc = bus.control_transfer(
///         &device,
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
/// # }
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

/// A device descriptor, see USB 2.0 section 9.6.1
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-8
#[allow(missing_docs)]
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

/// A configuration descriptor, see USB 2.0 section 9.6.3
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-10
#[allow(missing_docs)]
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

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ConfigurationDescriptor {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ConfigurationDescriptor {}

/// An interface descriptor, see USB 2.0 section 9.6.5
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-12
#[allow(missing_docs)]
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

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for InterfaceDescriptor {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for InterfaceDescriptor {}

/// An endpoint descriptor, see USB 2.0 section 9.6.6
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-13
#[allow(missing_docs)]
pub struct EndpointDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: [u8; 2],
    pub bInterval: u8,
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for EndpointDescriptor {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for EndpointDescriptor {}

/// A hub descriptor, see USB 2.0 section 11.23.2.1
#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 11-13
#[allow(missing_docs)]
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

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for HubDescriptor {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for HubDescriptor {}

// For request_type (USB 2.0 table 9-2)

/// Control transfer: device-to-host
pub const DEVICE_TO_HOST: u8 = 0x80;

/// Control transfer: host-to-device
pub const HOST_TO_DEVICE: u8 = 0;

/// Control transfer: request defined by USB standard
pub const STANDARD_REQUEST: u8 = 0;

/// Control transfer: request defined by USB class definition
pub const CLASS_REQUEST: u8 = 0x20;

/// Control transfer: request is vendor-specific
pub const VENDOR_REQUEST: u8 = 0x40;

/// Control transfer: request targets entire device
pub const RECIPIENT_DEVICE: u8 = 0;

/// Control transfer: request targets a particular interface
pub const RECIPIENT_INTERFACE: u8 = 1;

/// Control transfer: request targets a particular endpoing
pub const RECIPIENT_ENDPOINT: u8 = 2;

/// Control transfer: request targets something else
pub const RECIPIENT_OTHER: u8 = 3;

// For request (USB 2.0 table 9-4)

/// Request status (USB 2.0 section 9.4.5)
pub const GET_STATUS: u8 = 0;

/// Clear feature (USB 2.0 section 9.4.1)
pub const CLEAR_FEATURE: u8 = 1;

/// Set feature (USB 2.0 section 9.4.9)
pub const SET_FEATURE: u8 = 3;

/// Set address (USB 2.0 section 9.4.6)
pub const SET_ADDRESS: u8 = 5;

/// Get descriptor (USB 2.0 section 9.4.3)
pub const GET_DESCRIPTOR: u8 = 6;

/// Set descriptor (rarely used)
pub const SET_DESCRIPTOR: u8 = 7;

/// Set configuration (USB 2.0 section 9.4.7)
pub const SET_CONFIGURATION: u8 = 9;

// Descriptor types (USB 2.0 table 9-5)

/// Device descriptor (USB 2.0 section 9.6.1)
pub const DEVICE_DESCRIPTOR: u8 = 1;

/// Configuration descriptor (USB 2.0 section 9.6.3)
pub const CONFIGURATION_DESCRIPTOR: u8 = 2;

/// String descriptor (USB 2.0 section 9.6.7)
pub const STRING_DESCRIPTOR: u8 = 3;

/// Interface descriptor (USB 2.0 section 9.6.5)
pub const INTERFACE_DESCRIPTOR: u8 = 4;

/// Endpoint descriptor (USB 2.0 section 9.6.6)
pub const ENDPOINT_DESCRIPTOR: u8 = 5;

/// Hub descriptor (USB 2.0 section 11.23.3.1 and table 11-13)
pub const HUB_DESCRIPTOR: u8 = 0x29;

// Class codes (DeviceDescriptor.bDeviceClass)

/// Class code for USB hubs (USB 2.0 section 11.23.1)
pub const HUB_CLASSCODE: u8 = 9;

// Values for SET_FEATURE for hubs (USB 2.0 table 11-17)

/// Reset a port (USB 2.0 section 11.5.1.5)
pub const PORT_RESET: u16 = 4;

/// Power-on a port (USB 2.0 section 11.5.1.13)
pub const PORT_POWER: u16 = 8;

/// Endpoint type, see USB 2.0 sections 9.3.6 and 5.3.1
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum EndpointType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

/// Direction of a USB transfer
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    /// IN transactions are device-to-host transfers
    In,
    /// OUT transactions are host-to-device transfers
    Out,
}

/// Callbacks from [`parse_descriptors()`]
///
/// And hence from [`UsbBus::get_configuration()`](crate::usb_bus::UsbBus::get_configuration).
pub trait DescriptorVisitor {
    /// A configuration descriptor has been reported
    fn on_configuration(&mut self, _c: &ConfigurationDescriptor) {}

    /// An interface descriptor has been reported
    fn on_interface(&mut self, _i: &InterfaceDescriptor) {}

    /// An endpoint descriptor has been reported
    fn on_endpoint(&mut self, _e: &EndpointDescriptor) {}

    /// Some other descriptor has been reported (perhaps a vendor-defined one)
    fn on_other(&mut self, _d: &[u8]) {}
}

/// [`A DescriptorVisitor`] that just logs the descriptors to the debug stream
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

/// Parse a configuration-descriptor sequence
///
/// And make callbacks via the [`DescriptorVisitor`] for everything
/// that's found.
pub fn parse_descriptors(buf: &[u8], v: &mut impl DescriptorVisitor) {
    let mut index = 0;

    while buf.len() > index + 2 {
        let dlen = buf[index] as usize;
        let dtype = buf[index + 1];

        if dlen < 2 || buf.len() < index + dlen {
            return;
        }

        match dtype {
            CONFIGURATION_DESCRIPTOR => {
                if let Ok(c) =
                    bytemuck::try_from_bytes(&buf[index..index + dlen])
                {
                    v.on_configuration(c);
                }
            }
            INTERFACE_DESCRIPTOR => {
                if let Ok(i) =
                    bytemuck::try_from_bytes(&buf[index..index + dlen])
                {
                    v.on_interface(i);
                }
            }
            ENDPOINT_DESCRIPTOR => {
                if let Ok(e) =
                    bytemuck::try_from_bytes(&buf[index..index + dlen])
                {
                    v.on_endpoint(e);
                }
            }
            _ => v.on_other(&buf[index..(index + dlen)]),
        }

        index += dlen;
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/wire.rs"]
mod tests;
