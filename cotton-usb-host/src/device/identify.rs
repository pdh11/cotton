use crate::usb_bus::DeviceInfo;
use crate::wire::DescriptorVisitor;

pub trait IdentifyFromInfo {
    fn identify_from_info(info: &DeviceInfo) -> Option<u8>;
}

pub trait IdentifyFromDescriptors: DescriptorVisitor {
    fn identify(&self) -> Option<u8>;
}
