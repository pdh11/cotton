use core::future::Future;

/// The data phase of a SCSI transaction: in, out, or none
///
/// This is the same as the data phase of a USB transaction, but
/// that's kind-of a coincidence, so we duplicate the definition here
/// so as not to introduce a needless dependency.
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub enum DataPhase<'a> {
    /// The command involves data transfer from device to host
    In(&'a mut [u8]),
    /// The command involves data transfer from host to device
    Out(&'a [u8]),
    /// The command does not involve data transfer (the status response
    /// includes everything the host needs)
    None,
}

/// An abstract SCSI communications channel to a single device
///
/// An actual SCSI bus would implement one `ScsiTransport` for each
/// attached device-id; protocols where SCSI commands are tunnelled over
/// an outer protocol, typically already know that they are dealing with
/// exactly one "SCSI" device.
pub trait ScsiTransport {
    /// The type of errors which can arise from the transport itself: for
    /// instance, USB errors from a mass-storage class implementation.
    type Error: PartialEq + Eq;

    /// Execute one SCSI command
    ///
    /// The command is a byte slice containing the raw command block
    /// as specified by SCSI standards: for instance, a "READ(10)"
    /// command to a disk would be a 10-byte slice containing the
    /// structure in table 97 of the Seagate SCSI Commands Reference
    /// Manual.
    ///
    /// The "data" parameter encapsulates any input or output buffer for
    /// transferred data.
    ///
    /// Unless there was a host-side or transport-related error, the
    /// response will be either success or failure: this call does
    /// *not* itself issue a REQUEST SENSE command to determine
    /// exactly why a command failed. (But
    /// [`ScsiDevice::command_response()`](crate::scsi_device::ScsiDevice::command_response)
    /// *does* issue REQUEST SENSE, so it can report the wider range
    /// of errors seen in [`ScsiError`]).
    fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase,
    ) -> impl Future<Output = Result<usize, Error<Self::Error>>>;
}

/// Errors which can arise during a SCSI command
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error<T: PartialEq + Eq> {
    /// As an error from `ScsiTransport::command`: the device reported failure.
    /// As an error from `ScsiDevice::command_response`: the device reported
    /// failure, and none of the cases in `ScsiError` apply.
    CommandFailed,

    /// The device or transport seemed to deviate from the protocol spec.
    ProtocolError,

    /// The `ScsiTransport` itself (as opposed to the device) reported an error.
    Transport(T),

    /// The device experienced an error, as reported by REQUEST SENSE.
    Scsi(ScsiError),
}

/// Errors which can be returned over SCSI protocol from the SCSI device
///
/// As opposed to errors detected on the host such as transport errors.
///
/// See Seagate SCSI commands reference s2.4.1.5, 2.4.1.6
///
/// Many of these errors are obscure and/or catastrophic -- hopefully you
/// will never see `ScsiError::Overheat` -- but some are reasonable and
/// common.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum ScsiError {
    BecomingReady,
    StartUnitRequired,
    ManualInterventionRequired,
    FormatInProgress,
    SelfTestInProgress,
    PowerCycleRequired,
    Overheat,
    EnclosureDegraded,
    WriteError,
    WriteReallocationFailed,
    UnrecoveredReadError,
    ReadRetriesExhausted,
    ReadErrorTooLong,
    ReadReallocationFailed,
    LogicalBlockNotFound,
    RecordNotFound,
    InvalidFieldInParameterList,
    ParameterNotSupported,
    ParameterValueInvalid,
    LogicalUnitSelfTestFailed,
    SelfTestFailed,

    PositioningError,
    ParameterListLengthError,
    MiscompareDuringVerify,
    /// The device does not implement this command
    InvalidCommandOperationCode,
    LogicalBlockAddressOutOfRange,
    /// Something is incorrect in the command block itself
    InvalidFieldInCDB,
    LogicalUnitNotSupported,

    NotReady,
    MediumError,
    HardwareError,
    IllegalRequest,
    /// Something has happened to this device that means it should be
    /// re-evaluted (e.g. CD-ROM insertion or ejection)
    UnitAttention,
    /// A write was attempted to a read-only device (or similar)
    DataProtect,
    BlankCheck,
    VendorSpecific,
    CopyAborted,
    Aborted,
    VolumeOverflow,
    Miscompare,
}
