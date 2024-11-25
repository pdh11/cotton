[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-scsi)](https://crates.io/crates/cotton-scsi)
[![Crates.io](https://img.shields.io/crates/d/cotton-scsi)](https://crates.io/crates/cotton-scsi)
[![docs.rs](https://img.shields.io/docsrs/cotton-scsi)](https://docs.rs/cotton-scsi/latest/cotton_scsi/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-scsi

Part of the [Cotton](https://github.com/pdh11/cotton) project.

Actual SCSI hardware is rarely seen these days. But the command
protocols live on, and are important for USB mass-storage class (USB
storage devices) when tunnelled over USB and for CD-ROM when tunnelled
over ATAPI.

This crate so far implements only those commands important for "direct
storage access devices" (disks and flash-drives), but the mechanisms should
be generic to all SCSI commands, such as for optical drives.

The most accessible reference for SCSI commands for disks (or other
direct storage) is the "Seagate SCSI Commands Reference Manual" found
at
<https://www.seagate.com/files/staticfiles/support/docs/manual/Interface%20manuals/100293068j.pdf>
