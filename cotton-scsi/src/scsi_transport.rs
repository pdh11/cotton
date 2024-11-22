use core::future::Future;

/// The data phase of a SCSI transaction: in, out, or none
///
/// This is the same as the data phase of a USB transaction, but
/// that's kind-of a coincidence, so we duplicate the definition here
/// so as not to introduce a needless dependency.
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub enum DataPhase<'a> {
    In(&'a mut [u8]),
    Out(&'a [u8]),
    None,
}

pub trait ScsiTransport {
    type Error: PartialEq + Eq;

    fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase,
    ) -> impl Future<Output = Result<usize, Error<Self::Error>>>;
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error<T: PartialEq + Eq> {
    CommandFailed,
    ProtocolError,
    Transport(T),
    Scsi(ScsiError),
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScsiError {
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
