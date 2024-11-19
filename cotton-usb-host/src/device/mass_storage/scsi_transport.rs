use crate::host_controller::{DataPhase, UsbError};
use core::future::Future;

pub trait ScsiTransport {
    fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase,
    ) -> impl Future<Output = Result<usize, Error>>;
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    CommandFailed,
    ProtocolError,
    Usb(UsbError),

    // Seagate SCSI commands reference s2.4.1.5, 2.4.1.6
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
    InvalidCommandOperationCode,
    LogicalBlockAddressOutOfRange,
    InvalidFieldInCDB,
    LogicalUnitNotSupported,

    NotReady,
    MediumError,
    HardwareError,
    IllegalRequest,
    UnitAttention,
    DataProtect,
    BlankCheck,
    VendorSpecific,
    CopyAborted,
    Aborted,
    VolumeOverflow,
    Miscompare,
}

impl From<UsbError> for Error {
    fn from(e: UsbError) -> Self {
        Error::Usb(e)
    }
}
