use crate::bitset::{BitIterator, BitSet};
use crate::debug;
use crate::host_controller::{
    DeviceStatus, HostController, InterruptPacket, MultiInterruptPipe,
};
use crate::interrupt::{InterruptStream, MultiInterruptStream};
use crate::topology::Topology;
use crate::wire::{
    ConfigurationDescriptor, DescriptorVisitor, DeviceInfo,
    EndpointDescriptor, HubDescriptor, SetupPacket, UsbDevice, UsbError,
    UsbSpeed, CLASS_REQUEST, CLEAR_FEATURE, CONFIGURATION_DESCRIPTOR,
    DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, GET_STATUS,
    HOST_TO_DEVICE, HUB_CLASSCODE, HUB_DESCRIPTOR, PORT_POWER, PORT_RESET,
    RECIPIENT_OTHER, SET_ADDRESS, SET_CONFIGURATION, SET_FEATURE,
};
use core::cell::RefCell;
use futures::future::FutureExt;
use futures::{Stream, StreamExt};

pub use crate::host_controller::DataPhase;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
/// A device-related event has occurred.
///
/// This is the type of events returned from
/// [`UsbBus::device_events()`] or [`UsbBus::device_events_no_hubs`].
/// The important events are hot-plug ("`Connect`") and
/// hot-unplug("`Disconnect`"). Device events are how your code can
/// detect the presence of USB devices and start to communicate with
/// them.
///
pub enum DeviceEvent {
    /// A new device has been connected. It has been given an address,
    /// but has not yet been configured (state "Address" in USB 2.0
    /// figure 9-1). Your code can read the device's descriptors to
    /// confirm its identity and figure out what to do with it, but
    /// must then call [`UsbBus::configure()`] before communicating
    /// with it "for real".
    ///
    /// The `UsbDevice` object encapsulates the newly-assigned USB device
    /// address. Basic information about the device -- sufficient, perhaps, to
    /// select an appropriate driver from those available -- is in the
    /// supplied `DeviceInfo`.
    Connect(UsbDevice, DeviceInfo),

    /// A previously-reported device has become disconnected. This event
    /// includes a _set_ of affected devices -- if a hub has become
    /// disconnected, then every device downstream of it has simultaneously
    /// _also_ become disconnected.
    ///
    /// The devices are represented by a bitmap: bit N set means that USB
    /// device address N is part of this set.
    ///
    /// (So bit zero is never set, because 0 is never a valid assigned USB
    /// device address.)
    Disconnect(BitSet),

    /// A device appears to have been connected, but is not
    /// successfully responding to the mandatory enumeration commands.
    /// This usually indicates inadequate power supply, or perhaps
    /// damaged cabling.
    EnumerationError(u8, u8, UsbError),

    /// There is nothing currently to report. (This event is sometimes sent
    /// for internal reasons, and can be ignored.)
    None,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Default, PartialEq, Eq)]
pub struct BasicConfiguration {
    pub num_configurations: u8,
    pub configuration_value: u8,
    pub in_endpoints: u16,
    pub out_endpoints: u16,
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
    //needs_reset: BitSet,
}

/// A USB host bus.
///
/// This object represents the (portable) concept of a host's view of
/// a whole bus of USB devices. It is constructed from a
/// [`HostController`] object which encapsulates the driver for
/// specific USB-host-controller hardware.
///
/// Starting from a `UsbBus` object, you can obtain details of any USB
/// devices attached to this host, and start communicating with them as
/// needed.
///
/// Devices with multiple USB host controllers will require a `UsbBus`
/// object for each of them.
///
pub struct UsbBus<HC: HostController> {
    driver: HC,
    hub_pipes: RefCell<HC::MultiInterruptPipe>,
    hub_state: RefCell<HubState>,
}

impl<HC: HostController> UsbBus<HC> {
    /// Create a new USB host bus from a host-controller driver
    pub fn new(driver: HC) -> Self {
        let hp = driver.multi_interrupt_pipe();

        Self {
            driver,
            hub_pipes: RefCell::new(hp),
            hub_state: Default::default(),
        }
    }

    /// Obtain a stream of hotplug/hot-unplug events
    ///
    /// This stream is how the USB host stack informs your code that a
    /// USB device is available for use. Once you have [created a
    /// `UsbBus` object](UsbBus::new()), you can call `device_events()` and
    /// get a stream of [`DeviceEvent`] objects:
    ///
    /// ```no_run
    /// # use cotton_usb_host::host_controller::HostController;
    /// # use std::pin::{pin, Pin};
    /// # use cotton_usb_host::usb_bus::UsbBus;
    /// # use futures::{Stream, StreamExt};
    /// # async fn foo<D: HostController>(driver: D) -> () {
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events());
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # }
    /// ```
    ///
    /// When using this method, the cotton-usb-host crate itself takes
    /// care of detecting and configuring hubs, and of detecting
    /// devices downstream of hubs. At present, the hubs do themselves
    /// appear as `DeviceEvent`s, but your code doesn't need to do
    /// anything with them.
    ///
    /// If you know for a fact that your hardware setup does not
    /// include any hubs (or if you wish to operate the hubs
    /// yourself), you can use
    /// [`device_events_no_hubs()`](`UsbBus::device_events_no_hubs()`)
    /// instead of `device_events()` and get smaller, simpler code.
    ///
    pub fn device_events(&self) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();

        enum InternalEvent {
            Root(DeviceStatus),
            Packet(InterruptPacket),
        }

        futures::stream::select(
            root_device.map(InternalEvent::Root),
            MultiInterruptStream::<HC::MultiInterruptPipe> {
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
                                return DeviceEvent::EnumerationError(0, 1, e);
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
                        DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
                    }
                }
                InternalEvent::Packet(packet) => {
                    self.handle_hub_packet(&packet).await.unwrap_or_else(|e| {
                        DeviceEvent::EnumerationError(0, 1, e)
                    })
                }
            }
        })
    }

    /// Obtain a stream of hotplug/hot-unplug events
    ///
    /// This stream is how the USB host stack informs your code that a
    /// USB device is available for use. Once you have [created a
    /// `UsbBus` object](UsbBus::new()), you can call `device_events()` and
    /// get a stream of [`DeviceEvent`] objects:
    ///
    /// ```no_run
    /// # use cotton_usb_host::host_controller::HostController;
    /// # use std::pin::{pin, Pin};
    /// # use cotton_usb_host::usb_bus::UsbBus;
    /// # use futures::{Stream, StreamExt};
    /// # async fn foo<D: HostController>(driver: D) -> () {
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events_no_hubs());
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # }
    /// ```
    ///
    /// When using this method, the cotton-usb-host crate deals only with
    /// a single USB device attached directly to the USB host controller,
    /// i.e. that device is not treated specially if it is a hub.
    ///
    /// If you would rather let the cotton-usb-host crate take care of
    /// hubs automatically, you can use
    /// [`device_events()`](`UsbBus::device_events_no_hubs()`) instead
    /// of `device_events_no_hubs()`.
    ///
    pub fn device_events_no_hubs(
        &self,
    ) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();
        root_device.then(move |status| async move {
            if let DeviceStatus::Present(speed) = status {
                match self.new_device(speed).await {
                    Ok(info) => match self.set_address(1, &info).await {
                        Ok(device) => DeviceEvent::Connect(device, info),
                        Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                    },
                    Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                }
            } else {
                DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
            }
        })
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
        .map(|r| r.map(|_| ()))
        .await
    }

    async fn new_device(
        &self,
        speed: UsbSpeed,
    ) -> Result<DeviceInfo, UsbError> {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let sz = self
            .control_transfer(
                0,
                8,
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
        if sz < 8 {
            return Err(UsbError::ProtocolError);
        }

        let packet_size_ep0 = descriptors[7];

        // Fetch rest of device descriptor
        let sz = self
            .control_transfer(
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
        if sz < 18 {
            return Err(UsbError::ProtocolError);
        }

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
            InterruptStream::<HC::InterruptPipe<'_>> { pipe }
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
        crate::wire::parse_descriptors(&buf[0..sz], &mut bd);
        if bd.num_configurations == 0 || bd.configuration_value == 0 {
            Err(UsbError::ProtocolError)
        } else {
            Ok(bd)
        }
    }

    async fn new_hub(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
    ) -> Result<(), UsbError> {
        debug::println!("gbc!");
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

        if sz < core::mem::size_of::<HubDescriptor>() {
            return Err(UsbError::ProtocolError);
        }

        let ports = descriptors[2];
        debug::println!("{}-port hub", ports);

        // Ports are numbered from 1..=N (not 0..N)
        for port in 1..=ports {
            self.set_port_feature(device.address, port, PORT_POWER)
                .await?;
        }

        Ok(())
    }

    /// Return a snapshot of the current physical bus layout
    ///
    /// This snapshot includes a representation of all the hubs and
    /// devices currently detected, and how they are linked together.
    ///
    /// This is useful for logging/debugging.
    pub fn topology(&self) -> Topology {
        self.hub_state.borrow().topology.clone()
    }

    async fn get_hub_port_status(
        &self,
        hub_address: u8,
        port: u8,
    ) -> Result<(u16, u16), UsbError> {
        let mut data = [0u8; 4];
        self.control_transfer(
            hub_address,
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

        Ok((
            u16::from_le_bytes([data[0], data[1]]),
            u16::from_le_bytes([data[2], data[3]]),
        ))
    }

    /// Clear C_PORT_CONNECTION (or similar status-change bit); see
    /// USB 2.0 s11.24.2.7.2
    async fn clear_port_feature(
        &self,
        hub_address: u8,
        port: u8,
        feature: u16,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            hub_address,
            8,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE
                    | CLASS_REQUEST
                    | RECIPIENT_OTHER,
                bRequest: CLEAR_FEATURE,
                wValue: feature,
                wIndex: port as u16,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(())
    }

    async fn set_port_feature(
        &self,
        hub_address: u8,
        port: u8,
        feature: u16,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            hub_address,
            8,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE
                    | CLASS_REQUEST
                    | RECIPIENT_OTHER,
                bRequest: SET_FEATURE,
                wValue: feature,
                wIndex: port as u16,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(())
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

        if packet.size == 0 {
            return Err(UsbError::ProtocolError);
        }

        let mut port_bitmap = packet.data[0] as u32;
        if packet.size > 1 {
            port_bitmap |= (packet.data[1] as u32) << 8;
        }
        let port_bitmap = BitIterator::new(port_bitmap);
        for port in port_bitmap {
            debug::println!("I'm told to investigate port {}", port);

            let (state, changes) =
                self.get_hub_port_status(packet.address, port).await?;
            debug::println!(
                "  port {} status3 {:x} {:x}",
                port,
                state,
                changes
            );

            if changes != 0 {
                let bit = changes.trailing_zeros(); // i.e., least_set_bit

                if bit < 5 {
                    // "+16" to clear the change version C_xx rather than the
                    // feature itself, see USB 2.0 table 11-17
                    self.clear_port_feature(
                        packet.address,
                        port,
                        (bit + 16) as u16,
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
                        return Ok(DeviceEvent::Disconnect(BitSet(mask)));
                    }

                    // now connected
                    if self.hub_state.borrow().currently_resetting.is_none() {
                        self.set_port_feature(
                            packet.address,
                            port,
                            PORT_RESET,
                        )
                        .await?;
                        self.hub_state.borrow_mut().currently_resetting =
                            Some((packet.address, port));

                        // wait for the reset to complete (we'll be called
                        // again with C_PORT_RESET)
                        return Ok(DeviceEvent::None);
                    } else {
                        // TODO: queue hub for future investigation
                    }
                }
                if bit == 4 {
                    // C_PORT_RESET -- "This bit is set when the port
                    // transitions [...] to the Enabled state"

                    // USB 2.0 table 11-21
                    let speed = match state & 0x600 {
                        0 => UsbSpeed::Full12,
                        0x400 => UsbSpeed::High480,
                        _ => UsbSpeed::Low1_5,
                    };

                    let info = self.new_device(speed).await?;
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
                        self.new_hub(&device, &info).await?;
                    }

                    // TODO: can we reset another port now?

                    return Ok(DeviceEvent::Connect(device, info));
                }
            }
        }
        // TODO: if we get here, does some other port need resetting?
        Ok(DeviceEvent::None)
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/usb_bus.rs"]
mod tests;
