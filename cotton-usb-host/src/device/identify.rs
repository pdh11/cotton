use crate::host_controller::HostController;
use crate::usb_bus::{DeviceInfo, UnconfiguredDevice, UsbBus};

pub trait UsbIdentify<HC: HostController> {
    fn identify(
        bus: &UsbBus<HC>,
        device: &UnconfiguredDevice,
        info: &DeviceInfo,
    ) -> Option<u8>;
}
