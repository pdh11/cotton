use crate::usb_bus::DeviceInfo;
use crate::wire::DescriptorVisitor;

/// Trait for USB device drivers which can identify their target device from a [`DeviceInfo`] alone
///
/// Typically, that means device drivers which only work with specific VID/PID
/// combinations.
pub trait IdentifyFromInfo {
    /// Is this USB device capable of being driven by this driver?
    ///
    /// Returns:
    /// - `None`: no, it isn't
    /// - `Some(N)`: yes, it is -- _if_ configured using `UsbBus::configure(device, N)`
    fn identify_from_info(info: &DeviceInfo) -> Option<u8>;
}

/// Trait for USB device drivers which need to use USB descriptors to identify their target device
///
/// For instance, USB mass-storage class devices are recognised by a class code
/// in the Interface Descriptor.
pub trait IdentifyFromDescriptors: DescriptorVisitor {
    /// Is this USB device capable of being driven by this driver?
    ///
    /// Returns:
    /// - `None`: no, it isn't
    /// - `Some(N)`: yes, it is -- _if_ configured using `UsbBus::configure(device, N)`
    fn identify(&self) -> Option<u8>;
}
