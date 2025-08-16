use cotton_usb_host::{
    device::identify::IdentifyFromDescriptors,
    usb_bus::{HostController, UsbBus, UsbDevice, UsbError},
    wire::{ConfigurationDescriptor, DescriptorVisitor, InterfaceDescriptor},
};
use futures::{Stream, StreamExt};

use crate::debug;

pub struct Hid<'a, HC: HostController> {
    bus: &'a UsbBus<HC>,
    device: UsbDevice,
    in_ep: u8,
}

#[cfg(feature = "defmt")]
impl<HC> defmt::Format for Hid<'_, HC> where HC: HostController {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Hid(dev={}, ep={=u8})", self.device, self.in_ep);
    }
}

/// A report from our HID device
///
/// NB: Only supports 8-byte reports from a Boot mode keyboard.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct HidReport {
    pub bytes: [u8; 8],
}

impl<'a, HC: HostController> Hid<'a, HC> {
    const PKT_LEN: u16 = 8;
    const INTERVAL_MS: u8 = 10;

    pub fn new(
        bus: &'a UsbBus<HC>,
        device: UsbDevice,
    ) -> Result<Self, UsbError> {
        let in_ep = device.in_endpoints().iter().next().unwrap_or_default();
        Ok(Self { bus, device, in_ep })
    }

    /// Produce a stream of HID reports from a Boot-mode keyboard
    pub fn handle(&mut self) -> impl Stream<Item = HidReport> + '_ {
        self.bus
            .interrupt_endpoint_in(
                self.device.address(),
                self.in_ep,
                Self::PKT_LEN,
                Self::INTERVAL_MS,
            )
            .map(|pkt| HidReport {
                bytes: [
                    pkt.data[0],
                    pkt.data[1],
                    pkt.data[2],
                    pkt.data[3],
                    pkt.data[4],
                    pkt.data[5],
                    pkt.data[6],
                    pkt.data[7],
                ],
            })
    }
}

#[derive(Default)]
pub struct IdentifyHid {
    current_configuration: Option<u8>,
    hid_configuration: Option<u8>,
}

impl IdentifyHid {
    /// This is the bInterfaceClass for HID devices
    pub const INTERFACE_CLASS: u8 = 3;
    /// This is the bInterfaceProtocol for Keyboards
    pub const INTERFACE_PROTOCOL_KEYBOARD: u8 = 1;
    /// This subclass means the keyboard support 'Boot' mode, which is a simple
    /// mode designed for use with a PC BIOS (doing PS/2 emulation). This is
    /// the mode we want, because it means we can assume what the descriptor
    /// looks like without having to parse it.
    ///
    /// See https://www.usb.org/sites/default/files/hid1_11.pdf Appendix B.
    pub const INTERFACE_SUBCLASS_BOOT: u8 = 1;
}

impl DescriptorVisitor for IdentifyHid {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.current_configuration = Some(c.bConfigurationValue);
    }
    fn on_interface(&mut self, i: &InterfaceDescriptor) {
        match (
            i.bInterfaceClass,
            i.bInterfaceSubClass,
            i.bInterfaceProtocol,
        ) {
            (
                Self::INTERFACE_CLASS,
                Self::INTERFACE_SUBCLASS_BOOT,
                Self::INTERFACE_PROTOCOL_KEYBOARD,
            ) => {
                self.hid_configuration = self.current_configuration;
            }
            _ => {
                debug::println!(
                    "class {} subclass {} protocol {}",
                    i.bInterfaceClass,
                    i.bInterfaceSubClass,
                    i.bInterfaceProtocol
                );
            }
        }
    }
}

impl IdentifyFromDescriptors for IdentifyHid {
    fn identify(&self) -> Option<u8> {
        self.hid_configuration
    }
}
