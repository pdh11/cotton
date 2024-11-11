
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
fn hub() {
    let h: &HubDescriptor = bytemuck::from_bytes(HUB);
    assert_eq!(h.bNbrPorts, 4);
    assert_eq!(h.bHubContrCurrent, 100);
}

#[test]
fn invalid_descriptors() {
    // Mostly a test for Miri
    parse_descriptors(&[9, 41, 1], &mut ShowDescriptors);
    parse_descriptors(&[3, 2, 1], &mut ShowDescriptors);
    parse_descriptors(&[3, 4, 1], &mut ShowDescriptors);
    parse_descriptors(&[3, 5, 1], &mut ShowDescriptors);
}

#[test]
fn reserved_descriptor() {
    // Mostly a test for Miri
    parse_descriptors(&[3, 96, 1], &mut ShowDescriptors);
}
