use crate::debug;
use crate::host::rp2040::{DeviceDetect, UsbShared}; // fixme
use crate::host_controller::{
    DeviceStatus, HostController, InterruptPacket, MultiInterruptPipe,
};
use crate::interrupt::{InterruptStream, MultiInterruptStream};
use crate::types::{
    ConfigurationDescriptor, DescriptorVisitor, EndpointDescriptor,
    HubDescriptor, CLASS_REQUEST, CONFIGURATION_DESCRIPTOR, DEVICE_DESCRIPTOR,
    DEVICE_TO_HOST, GET_DESCRIPTOR, HOST_TO_DEVICE, HUB_CLASSCODE,
    HUB_DESCRIPTOR, PORT_POWER, RECIPIENT_OTHER, SET_ADDRESS,
    SET_CONFIGURATION, SET_FEATURE,
};
use crate::types::{SetupPacket, UsbDevice, UsbError, UsbSpeed};
use core::cell::RefCell;
use futures::future::FutureExt;
use futures::Stream;
use futures::StreamExt;

pub use crate::host_controller::DataPhase;

pub struct UsbDeviceSet(pub u32);

impl UsbDeviceSet {
    pub fn contains(&self, n: u8) -> bool {
        (self.0 & (1 << n)) != 0
    }
}

pub enum DeviceEvent {
    Connect(UsbDevice),
    Disconnect(UsbDeviceSet),
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Default)]
pub struct BasicConfiguration {
    configuration_value: u8,
    in_endpoints: u16,
    out_endpoints: u16,
}

impl DescriptorVisitor for BasicConfiguration {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.configuration_value = c.bConfigurationValue;
    }
    fn on_endpoint(&mut self, i: &EndpointDescriptor) {
        if (i.bEndpointAddress & 0x80) == 0x80 {
            self.in_endpoints |= 1 << (i.bEndpointAddress & 15);
        } else {
            self.out_endpoints |= 1 << (i.bEndpointAddress & 15);
        }
    }
}

pub struct UsbBus<HC: HostController> {
    driver: HC,
    shared: &'static UsbShared,
    hub_pipes: RefCell<HC::MultiInterruptPipe>,
}

impl<HC: HostController> UsbBus<HC> {
    pub fn new(driver: HC, shared: &'static UsbShared) -> Self {
        let hp = driver.multi_interrupt_pipe();

        Self {
            driver,
            shared,
            hub_pipes: RefCell::new(hp),
        }
    }

    pub async fn configure(
        &self,
        device: &UsbDevice,
        configuration_value: u8,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            device.address,
            device.packet_size_ep0,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE,
                bRequest: SET_CONFIGURATION,
                wValue: configuration_value as u16,
                wIndex: 0,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(())
    }

    async fn new_device(&self, speed: UsbSpeed, address: u8) -> UsbDevice {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let rc = self
            .control_transfer(
                0,
                64,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 8,
                },
                DataPhase::In(&mut descriptors),
            )
            .await;

        let packet_size_ep0 = if rc.is_ok() { descriptors[7] } else { 8 };

        // Set address
        let _ = self
            .control_transfer(
                0,
                packet_size_ep0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE,
                    bRequest: SET_ADDRESS,
                    wValue: address as u16,
                    wIndex: 0,
                    wLength: 0,
                },
                DataPhase::None,
            )
            .await;

        // Fetch rest of device descriptor
        let mut vid = 0;
        let mut pid = 0;
        let rc = self
            .control_transfer(
                1,
                packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 18,
                },
                DataPhase::In(&mut descriptors),
            )
            .await;
        if let Ok(_sz) = rc {
            vid = u16::from_le_bytes([descriptors[8], descriptors[9]]);
            pid = u16::from_le_bytes([descriptors[10], descriptors[11]]);
        } else {
            defmt::println!("Dtor fetch 2 {:?}", rc);
        }

        UsbDevice {
            address,
            packet_size_ep0,
            vid,
            pid,
            speed,
            class: descriptors[4],
            subclass: descriptors[5],
        }
    }

    pub async fn control_transfer<'a>(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'a>,
    ) -> Result<usize, UsbError> {
        self.driver
            .control_transfer(address, packet_size, setup, data_phase)
            .await
    }

    pub fn interrupt_endpoint_in(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval: u8,
    ) -> impl Stream<Item = InterruptPacket> + '_ {
        let pipe = self.driver.alloc_interrupt_pipe(
            address,
            endpoint,
            max_packet_size,
            interval,
        );
        async move {
            let pipe = pipe.await;
            InterruptStream::<HC> { pipe }
        }
        .flatten_stream()
    }

    pub async fn get_basic_configuration(
        &self,
        device: &UsbDevice,
    ) -> Result<BasicConfiguration, UsbError> {
        // TODO: descriptor suites >64 byte (Ella!)
        let mut buf = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 64,
                },
                DataPhase::In(&mut buf),
            )
            .await?;
        let mut bd = BasicConfiguration::default();
        crate::types::parse_descriptors(&buf[0..sz], &mut bd);
        Ok(bd)
    }

    pub fn device_events_no_hubs(
        &self,
    ) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = DeviceDetect::new(&self.shared.device_waker);
        root_device.then(move |status| async move {
            if let DeviceStatus::Present(speed) = status {
                let device = self.new_device(speed, 1).await;
                DeviceEvent::Connect(device)
            } else {
                DeviceEvent::Disconnect(UsbDeviceSet(0xFFFF_FFFF))
            }
        })
    }

    async fn new_hub(&self, device: &UsbDevice) -> Result<(), UsbError> {
        let bc = self.get_basic_configuration(device).await?;
        debug::println!("cfg: {:?}", &bc);
        self.configure(device, bc.configuration_value).await?;
        self.hub_pipes.borrow_mut().try_add(
            device.address,
            bc.in_endpoints.trailing_zeros() as u8,
            device.packet_size_ep0,
            9,
        )?;

        let mut descriptors = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | CLASS_REQUEST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: (HUB_DESCRIPTOR as u16) << 8,
                    wIndex: 0,
                    wLength: 64,
                },
                DataPhase::In(&mut descriptors),
            )
            .await?;

        let ports = if sz >= core::mem::size_of::<HubDescriptor>() {
            defmt::println!(
                "{}",
                &HubDescriptor::try_from_bytes(&descriptors[0..sz]).unwrap()
            );
            descriptors[2]
        } else {
            4
        };
        defmt::println!("{}-port hub", ports);

        // Ports are numbered from 1..=N (not 0..N)
        for port in 1..=ports {
            self.control_transfer(
                device.address,
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE
                        | CLASS_REQUEST
                        | RECIPIENT_OTHER,
                    bRequest: SET_FEATURE,
                    wValue: PORT_POWER,
                    wIndex: port as u16,
                    wLength: 0,
                },
                DataPhase::None,
            )
            .await?;
        }

        Ok(())
    }

    pub fn device_events(&self) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = DeviceDetect::new(&self.shared.device_waker);

        enum InternalEvent {
            Root(DeviceStatus),
            Packet(InterruptPacket),
        }

        futures::stream::select(
            root_device.map(InternalEvent::Root),
            MultiInterruptStream::<HC> {
                pipe: &self.hub_pipes,
            }
            .map(InternalEvent::Packet),
        )
        .then(move |ev| async move {
            match ev {
                InternalEvent::Root(status) => {
                    if let DeviceStatus::Present(speed) = status {
                        let device = self.new_device(speed, 1).await;
                        if device.class == HUB_CLASSCODE {
                            debug::println!("It's a hub");
                            let rc = self.new_hub(&device).await;
                            debug::println!("Hub startup: {:?}", rc);
                        }
                        DeviceEvent::Connect(device)
                    } else {
                        DeviceEvent::Disconnect(UsbDeviceSet(0xFFFF_FFFF))
                    }
                }
                InternalEvent::Packet(_packet) => {
                    todo!();
                }
            }
        })
    }
}
