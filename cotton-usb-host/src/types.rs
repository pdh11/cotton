use crate::debug;

#[repr(C)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-2
pub struct SetupPacket {
    pub bmRequestType: u8,
    pub bRequest: u8,
    pub wValue: u16,
    pub wIndex: u16,
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
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-10
pub struct ConfigurationDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    wTotalLength: [u8; 2],
    bNumInterfaces: u8,
    bConfigurationValue: u8,
    iConfiguration: u8,
    bmAttributes: u8,
    bMaxPower: u8,
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
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-12
pub struct InterfaceDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    bInterfaceNumber: u8,
    bAlternateSetting: u8,
    bNumEndpoints: u8,
    bInterfaceClass: u8,
    bInterfaceSubClass: u8,
    bInterfaceProtocol: u8,
    iInterface: u8,
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
#[derive(Copy, Clone)]
#[allow(non_snake_case)] // These names are from USB 2.0 table 9-13
pub struct EndpointDescriptor {
    bLength: u8,
    bDescriptorType: u8,
    bEndpointAddress: u8,
    bmAttributes: u8,
    wMaxPacketSize: [u8; 2],
    bInterval: u8,
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

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UsbError {
    Nak,
    Stall,
    Timeout,
    Overflow,
    BitStuffError,
    CrcError,
    DataSeqError,
    BufferTooSmall,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UsbSpeed {
    Low1_1,
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
#[derive(Copy, Clone)]
pub struct UsbDevice {
    pub address: u8,
    pub packet_size_ep0: u8,
    pub vid: u16,
    pub pid: u16,
    pub speed: UsbSpeed,
}

pub fn show_descriptors(buf: &[u8]) {
    let mut index = 0;

    while buf.len() > index + 2 {
        let dlen = buf[index] as usize;
        let dtype = buf[index + 1];

        if buf.len() < index + dlen {
            debug::println!("{}-byte dtor truncated", dlen);
            return;
        }

        match dtype {
            CONFIGURATION_DESCRIPTOR => {
                let c = ConfigurationDescriptor::try_from_bytes(
                    &buf[index..index + dlen],
                )
                .unwrap();
                debug::println!("  {:?}", c);
            }
            INTERFACE_DESCRIPTOR => {
                debug::println!(
                    "  {:?}",
                    InterfaceDescriptor::try_from_bytes(
                        &buf[index..index + dlen]
                    )
                    .unwrap()
                );
            }
            ENDPOINT_DESCRIPTOR => {
                debug::println!(
                    "  {:?}",
                    EndpointDescriptor::try_from_bytes(
                        &buf[index..index + dlen]
                    )
                    .unwrap()
                );
            }
            HUB_DESCRIPTOR => {
                debug::println!(
                    "  {:?}",
                    HubDescriptor::try_from_bytes(
                        &buf[index..index + dlen]
                    )
                    .unwrap()
                );
            }
            _ => {
                debug::println!("  type {} len {} skipped", dtype, dlen);
            }
        }

        index += dlen;
    }
}
