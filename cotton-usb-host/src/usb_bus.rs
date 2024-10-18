use crate::debug;
use crate::host_controller::{
    DeviceStatus, HostController, InterruptPacket, MultiInterruptPipe,
};
use crate::interrupt::{InterruptStream, MultiInterruptStream};
use crate::topology::Topology;
use crate::types::{
    ConfigurationDescriptor, DescriptorVisitor, EndpointDescriptor,
    HubDescriptor, CLASS_REQUEST, CLEAR_FEATURE, CONFIGURATION_DESCRIPTOR,
    DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, GET_STATUS,
    HOST_TO_DEVICE, HUB_CLASSCODE, HUB_DESCRIPTOR, PORT_POWER, PORT_RESET,
    RECIPIENT_OTHER, SET_ADDRESS, SET_CONFIGURATION, SET_FEATURE,
};
use crate::types::{DeviceInfo, SetupPacket, UsbDevice, UsbError, UsbSpeed};
use core::cell::RefCell;
use futures::future::FutureExt;
use futures::Stream;
use futures::StreamExt;

pub use crate::host_controller::DataPhase;

#[derive(Default)]
pub struct UsbDeviceSet(pub u32);

impl UsbDeviceSet {
    pub fn contains(&self, n: u8) -> bool {
        (self.0 & (1 << n)) != 0
    }
}

pub enum DeviceEvent {
    Connect(UsbDevice, DeviceInfo),
    Disconnect(UsbDeviceSet),
    EnumerationError(u8, u8, UsbError),
    None,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Default)]
pub struct BasicConfiguration {
    num_configurations: u8,
    configuration_value: u8,
    in_endpoints: u16,
    out_endpoints: u16,
}

impl DescriptorVisitor for BasicConfiguration {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.num_configurations += 1;
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

#[derive(Default)]
struct HubState {
    topology: Topology,
    currently_resetting: Option<(u8, u8)>,
    //needs_reset: UsbDeviceSet,
}

pub struct UsbBus<HC: HostController> {
    driver: HC,
    hub_pipes: RefCell<HC::MultiInterruptPipe>,
    hub_state: RefCell<HubState>,
}

impl<HC: HostController> UsbBus<HC> {
    pub fn new(driver: HC) -> Self {
        let hp = driver.multi_interrupt_pipe();

        Self {
            driver,
            hub_pipes: RefCell::new(hp),
            hub_state: Default::default(),
        }
    }

    pub async fn configure(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
        configuration_value: u8,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            device.address,
            info.packet_size_ep0,
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

    async fn new_device(
        &self,
        speed: UsbSpeed,
    ) -> Result<DeviceInfo, UsbError> {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        self.control_transfer(
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
        .await?;

        let packet_size_ep0 = descriptors[7];

        // Fetch rest of device descriptor
        self.control_transfer(
            0,
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
        .await?;
        let vid = u16::from_le_bytes([descriptors[8], descriptors[9]]);
        let pid = u16::from_le_bytes([descriptors[10], descriptors[11]]);

        Ok(DeviceInfo {
            packet_size_ep0,
            vid,
            pid,
            speed,
            class: descriptors[4],
            subclass: descriptors[5],
        })
    }

    async fn set_address(
        &self,
        address: u8,
        info: &DeviceInfo,
    ) -> Result<UsbDevice, UsbError> {
        self.control_transfer(
            0,
            info.packet_size_ep0,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE,
                bRequest: SET_ADDRESS,
                wValue: address as u16,
                wIndex: 0,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(UsbDevice { address })
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
        info: &DeviceInfo,
    ) -> Result<BasicConfiguration, UsbError> {
        // TODO: descriptor suites >64 byte (Ella!)
        let mut buf = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                info.packet_size_ep0,
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
        let root_device = self.driver.device_detect();
        root_device.then(move |status| async move {
            if let DeviceStatus::Present(speed) = status {
                if let Ok(info) = self.new_device(speed).await {
                    if let Ok(device) = self.set_address(1, &info).await {
                        return DeviceEvent::Connect(device, info);
                    }
                }
                // Can't enumerate device
                DeviceEvent::Disconnect(UsbDeviceSet(0))
            } else {
                DeviceEvent::Disconnect(UsbDeviceSet(0xFFFF_FFFF))
            }
        })
    }

    async fn new_hub(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
    ) -> Result<(), UsbError> {
        let bc = self.get_basic_configuration(device, info).await?;
        debug::println!("cfg: {:?}", &bc);
        self.configure(device, info, bc.configuration_value).await?;
        self.hub_pipes.borrow_mut().try_add(
            device.address,
            bc.in_endpoints.trailing_zeros() as u8,
            info.packet_size_ep0,
            9,
        )?;

        let mut descriptors = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                info.packet_size_ep0,
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
            debug::println!(
                "{:?}",
                &HubDescriptor::try_from_bytes(&descriptors[0..sz]).unwrap()
            );
            descriptors[2]
        } else {
            4
        };
        debug::println!("{}-port hub", ports);

        // Ports are numbered from 1..=N (not 0..N)
        for port in 1..=ports {
            self.control_transfer(
                device.address,
                info.packet_size_ep0,
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

    pub fn topology(&self) -> Topology {
        self.hub_state.borrow().topology.clone()
    }

    async fn handle_hub_packet(
        &self,
        packet: &InterruptPacket,
    ) -> Result<DeviceEvent, UsbError> {
        // Hub state machine: each hub must have each port powered,
        // then reset. But only one hub port on the whole *bus* can be
        // in reset at any one time, because it becomes sensitive to
        // address zero. So there needs to be a bus-wide hub state
        // machine.
        //
        // Idea: hubs get given addresses 1-15, so a bitmap of hubs
        // fits in u16. Newly found hubs get their interrupt EP added
        // to the waker, and all their ports powered. Ports that see a
        // C_PORT_CONNECTION get the parent hub added to a bitmap of
        // "hubs needing resetting". When no port is currently in
        // reset, a hub is removed from the bitmap, and a port counter
        // (1..=N) is set up; each port in turn has GetStatus called,
        // and on returning CONNECTION but not ENABLE, the port is
        // reset. (Not connected or already enabled means progress to
        // the next port, then the next hub.)
        //
        // On a reset complete, give the new device an address (1-15
        // if it, too, is a hub) and then again proceed to the next
        // port, then the next hub.

        debug::println!(
            "Hub int {} [{}; {}]",
            packet.address,
            packet.data[0],
            packet.size
        );

        let mut data = [0u8; 8];
        let sz = self
            .control_transfer(
                packet.address,
                8,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | CLASS_REQUEST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: (HUB_DESCRIPTOR as u16) << 8,
                    wIndex: 0,
                    wLength: 7,
                },
                DataPhase::In(&mut data),
            )
            .await?;

        if sz < 3 {
            return Err(UsbError::ProtocolError);
        }

        let mut port_bitmap = packet.data[0] as u32;
        if packet.size > 1 {
            port_bitmap |= (packet.data[1] as u32) << 8;
        }
        let port_bitmap = crate::async_pool::BitIterator::new(port_bitmap);
        for port in port_bitmap {
            debug::println!("I'm told to investigate port {}", port);

            self.control_transfer(
                packet.address,
                8,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST
                        | CLASS_REQUEST
                        | RECIPIENT_OTHER,
                    bRequest: GET_STATUS,
                    wValue: 0,
                    wIndex: port as u16,
                    wLength: 4,
                },
                DataPhase::In(&mut data),
            )
            .await?;

            let state = u16::from_le_bytes([data[0], data[1]]);
            let changes = u16::from_le_bytes([data[2], data[3]]);

            debug::println!(
                "  port {} status3 {:x} {:x}",
                port,
                state,
                changes
            );

            if changes != 0 {
                let bit = changes.trailing_zeros(); // i.e., least_set_bit

                if bit < 8 {
                    // Clear C_PORT_CONNECTION (or similar
                    // status-change bit); see USB 2.0 s11.24.2.7.2
                    self.control_transfer(
                        packet.address,
                        8,
                        SetupPacket {
                            bmRequestType: HOST_TO_DEVICE
                                | CLASS_REQUEST
                                | RECIPIENT_OTHER,
                            bRequest: CLEAR_FEATURE,
                            wValue: (bit as u16) + 16,
                            wIndex: port as u16,
                            wLength: 0,
                        },
                        DataPhase::None,
                    )
                    .await?;
                }
                if bit == 0 {
                    // C_PORT_CONNECTION
                    if (state & 1) == 0 {
                        // now disconnected
                        let mask = self
                            .hub_state
                            .borrow_mut()
                            .topology
                            .device_disconnect(packet.address, port);
                        return Ok(DeviceEvent::Disconnect(UsbDeviceSet(
                            mask,
                        )));
                    }

                    // now connected
                    if self.hub_state.borrow().currently_resetting.is_none() {
                        self.hub_state.borrow_mut().currently_resetting =
                            Some((packet.address, port));
                        self.control_transfer(
                            packet.address,
                            8,
                            SetupPacket {
                                bmRequestType: HOST_TO_DEVICE
                                    | CLASS_REQUEST
                                    | RECIPIENT_OTHER,
                                bRequest: SET_FEATURE,
                                wValue: PORT_RESET,
                                wIndex: port as u16,
                                wLength: 0,
                            },
                            DataPhase::None,
                        )
                        .await?;
                        return Ok(DeviceEvent::Disconnect(UsbDeviceSet(0)));
                    } else {
                        // TODO: queue hub for future investigation
                    }
                }
                if bit == 4 {
                    // C_PORT_RESET

                    // USB 2.0 table 11-21
                    let speed = match state & 0x600 {
                        0 => UsbSpeed::Full12,
                        0x200 => UsbSpeed::Low1_5,
                        0x400 => UsbSpeed::High480,
                        _ => UsbSpeed::Low1_5, // actually undefined
                    };

                    let Ok(info) = self.new_device(speed).await else {
                        return Ok(DeviceEvent::Disconnect(UsbDeviceSet(0)));
                    };
                    let is_hub = info.class == HUB_CLASSCODE;
                    let address = {
                        let mut state = self.hub_state.borrow_mut();
                        state.currently_resetting = None;
                        state
                            .topology
                            .device_connect(packet.address, port, is_hub)
                            .ok_or(UsbError::TooManyDevices)?
                    };
                    let device = self.set_address(address, &info).await?;
                    if is_hub {
                        debug::println!("It's a hub");
                        let rc = self.new_hub(&device, &info).await;
                        debug::println!("Hub startup: {:?}", rc);
                    }
                    return Ok(DeviceEvent::Connect(device, info));
                }
            }
        }
        // TODO: if we get here, does some other port need resetting?
        Ok(DeviceEvent::Disconnect(UsbDeviceSet(0)))
    }

    pub fn device_events(&self) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();

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
                        let info = match self.new_device(speed).await {
                            Ok(info) => info,
                            Err(e) => {
                                return DeviceEvent::EnumerationError(0, 1, e)
                            }
                        };
                        let is_hub = info.class == HUB_CLASSCODE;
                        let address = self
                            .hub_state
                            .borrow_mut()
                            .topology
                            .device_connect(0, 1, is_hub)
                            .expect("Root connect should always succeed");
                        let device = match self
                            .set_address(address, &info)
                            .await
                        {
                            Ok(device) => device,
                            Err(e) => {
                                return DeviceEvent::EnumerationError(0, 1, e)
                            }
                        };
                        if is_hub {
                            debug::println!("It's a hub");
                            match self.new_hub(&device, &info).await {
                                Ok(()) => (),
                                Err(e) => {
                                    return DeviceEvent::EnumerationError(
                                        0, 1, e,
                                    )
                                }
                            };
                        }
                        DeviceEvent::Connect(device, info)
                    } else {
                        self.hub_state
                            .borrow_mut()
                            .topology
                            .device_disconnect(0, 1);
                        DeviceEvent::Disconnect(UsbDeviceSet(0xFFFF_FFFF))
                    }
                }
                InternalEvent::Packet(packet) => self
                    .handle_hub_packet(&packet)
                    .await
                    .unwrap_or(DeviceEvent::Disconnect(UsbDeviceSet(0))),
            }
        })
    }
}
