// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The drive itself: listing, table of contents, error classes and ejecting.
//! Nothing here assumes a device name; paths come from the system or from the caller.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use cd_da_reader::{CdReader, CdReaderError, Toc};

use crate::discid::{TocInfo, TocTrack};

/// `CDROM_DRIVE_STATUS` ioctl (linux/cdrom.h).
const CDROM_DRIVE_STATUS: libc::c_ulong = 0x5326;
/// Argument of `CDROM_DRIVE_STATUS` meaning "the drive itself, not a slot of a changer" (`CDSL_CURRENT`, `INT_MAX`).
const CDSL_CURRENT: libc::c_int = i32::MAX;
/// Drive status values (linux/cdrom.h).
const CDS_NO_DISC: libc::c_int = 1;
const CDS_TRAY_OPEN: libc::c_int = 2;
const CDS_DISC_OK: libc::c_int = 4;
/// `CDROMEJECT` ioctl. It toggles on some drives, so it is only sent while a disc is known to be in.
const CDROMEJECT: libc::c_ulong = 0x5309;
const CDROM_LOCKDOOR: libc::c_ulong = 0x5329;
/// `SG_IO` ioctl.
const SG_IO: libc::c_ulong = 0x2285;
/// `O_NONBLOCK`, so opening a drive never waits for a disc.
const O_NONBLOCK: i32 = 0o4000;

/// Why a drive command failed, as far as the helper can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The drive is gone, the tray is open or the disc was removed.
    DriveGone,
    /// The disc could not be read at that place.
    Read,
}

/// Sorts an error of the reader: no device, no medium, "not ready" mean the drive state changed;
/// everything else is a read problem.
pub fn classify(e: &CdReaderError) -> ErrorClass {
    match e {
        CdReaderError::Io(io) => match io.raw_os_error() {
            Some(libc::ENODEV) | Some(libc::ENXIO) | Some(libc::ENOMEDIUM) | Some(libc::EBADF) => ErrorClass::DriveGone,
            _ if io.kind() == std::io::ErrorKind::NotFound => ErrorClass::DriveGone,
            _ => ErrorClass::Read,
        },
        CdReaderError::Scsi(s) => {
            // Sense key 2 = NOT READY; ASC 0x3A = MEDIUM NOT PRESENT.
            if s.sense_key == Some(2) || s.asc == Some(0x3A) { ErrorClass::DriveGone } else { ErrorClass::Read }
        }
        CdReaderError::NoUsableDrive => ErrorClass::DriveGone,
        _ => ErrorClass::Read,
    }
}

/// What the kernel says about a drive (`CDROM_DRIVE_STATUS`), without reading anything from the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveStatus {
    TrayOpen,
    NoDisc,
    /// The drive is busy (spinning up, closing the tray): look again in a moment.
    NotReady,
    DiscOk,
    /// The drive cannot tell.
    Unknown,
}

/// Asks the kernel for the state of the tray and disc. Opens the device without waiting for a disc and
/// sends no command to the drive that could move the tray. The error is the one of opening (`PermissionDenied`
/// for a drive the process may not use, `NotFound` for one that is gone).
pub fn status(device: &str) -> std::io::Result<DriveStatus> {
    let f = OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(device)?;
    // SAFETY: plain ioctl with an integer argument on an open descriptor.
    let s = unsafe { libc::ioctl(f.as_raw_fd(), CDROM_DRIVE_STATUS as _, CDSL_CURRENT) };
    Ok(match s {
        CDS_NO_DISC => DriveStatus::NoDisc,
        CDS_TRAY_OPEN => DriveStatus::TrayOpen,
        3 => DriveStatus::NotReady, // CDS_DRIVE_NOT_READY
        CDS_DISC_OK => DriveStatus::DiscOk,
        _ => DriveStatus::Unknown,
    })
}

/// `CDROM_MEDIA_CHANGED` ioctl: 1 if the disc was changed (or the tray opened) since the last time somebody asked.
const CDROM_MEDIA_CHANGED: libc::c_ulong = 0x5325;

/// Has the medium changed since the last call (by anyone)? Cheap: no command goes to the drive's optics. `None`
/// if the drive cannot tell.
pub fn media_changed(device: &str) -> Option<bool> {
    let f = OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(device).ok()?;
    // SAFETY: plain ioctl with an integer argument on an open descriptor.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), CDROM_MEDIA_CHANGED as _, CDSL_CURRENT) };
    (r >= 0).then_some(r == 1)
}

/// The optical drives the system knows, as device paths (`/dev/sr0`, ...): the block devices named `sr<number>` in
/// sysfs (the kernel's name for SCSI/SATA/USB CD drives), without opening any of them. The paths come from the
/// system; nothing else about them is assumed.
pub fn devices() -> Vec<String> {
    devices_in(std::path::Path::new("/sys/class/block"))
}

/// The same for another sysfs folder (tests).
pub fn devices_in(sys_block: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(sys_block)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.strip_prefix("sr").is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|n| n[2..].parse::<u32>().unwrap_or(u32::MAX));
    names.into_iter().map(|n| format!("/dev/{n}")).collect()
}

/// The optical drives and whether each holds an audio CD (`(path, has_audio_cd)`).
pub fn list() -> Result<Vec<(String, bool)>, CdReaderError> {
    Ok(CdReader::list_drives()?.into_iter().map(|d| (d.path, d.has_audio_cd)).collect())
}

/// The table of contents in the form the disc id needs.
pub fn toc_info(toc: &Toc) -> TocInfo {
    TocInfo {
        tracks: toc.tracks.iter().map(|t| TocTrack { number: t.number, start_lba: t.start_lba, is_audio: t.is_audio }).collect(),
        leadout_lba: toc.leadout_lba,
    }
}

/// Length of each audio track in sectors (to the next track, or the end of the audio area).
pub fn track_sectors(info: &TocInfo) -> Vec<(u8, u32)> {
    let audio_end = info.audio_leadout();
    let mut out = Vec::new();
    let all = &info.tracks;
    for (i, t) in all.iter().enumerate().filter(|(_, t)| t.is_audio) {
        let end = match all.get(i + 1) {
            Some(n) if n.is_audio => n.start_lba,
            _ => audio_end,
        };
        out.push((t.number, end.saturating_sub(t.start_lba)));
    }
    out
}

/// Sends a command without data through `SG_IO`; true if the drive reports good status.
fn sg_cmd(fd: i32, cdb: &[u8]) -> bool {
    #[repr(C)]
    struct SgIoHdr {
        interface_id: i32,
        dxfer_direction: i32,
        cmd_len: u8,
        mx_sb_len: u8,
        iovec_count: u16,
        dxfer_len: u32,
        dxferp: *mut u8,
        cmdp: *const u8,
        sbp: *mut u8,
        timeout: u32,
        flags: u32,
        pack_id: i32,
        usr_ptr: *mut u8,
        status: u8,
        masked_status: u8,
        msg_status: u8,
        sb_len_wr: u8,
        host_status: u16,
        driver_status: u16,
        resid: i32,
        duration: u32,
        info: u32,
    }
    let mut sense = [0u8; 32];
    let mut h = SgIoHdr {
        interface_id: b'S' as i32,
        dxfer_direction: -1, // SG_DXFER_NONE
        cmd_len: cdb.len() as u8,
        mx_sb_len: 32,
        iovec_count: 0,
        dxfer_len: 0,
        dxferp: std::ptr::null_mut(),
        cmdp: cdb.as_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: 10_000,
        flags: 0,
        pack_id: 0,
        usr_ptr: std::ptr::null_mut(),
        status: 0,
        masked_status: 0,
        msg_status: 0,
        sb_len_wr: 0,
        host_status: 0,
        driver_status: 0,
        resid: 0,
        duration: 0,
        info: 0,
    };
    // SAFETY: `h` is a valid sg_io_hdr that lives through the call, as do the buffers it points to.
    let rc = unsafe { libc::ioctl(fd, SG_IO as _, &mut h as *mut SgIoHdr) };
    rc == 0 && h.status == 0 && h.host_status == 0 && h.driver_status == 0
}

/// Bytes per second of audio on a CD, in kilobytes (the unit of "1x": 176 kB/s).
pub const KB_PER_SECOND_1X: u32 = 176;

/// `SET CD-ROM SPEED` (MMC 0xBB) through `SG_IO` on the drive; `read_speed_kbs` is in kilobytes per second, 0xFFFF
/// means "as fast as the drive can".
fn set_cd_speed(device: &str, read_speed_kbs: u16) -> std::io::Result<()> {
    // SG_IO needs the device open for reading and writing (as `cd-da-reader` opens it).
    let f = OpenOptions::new().read(true).write(true).custom_flags(O_NONBLOCK).open(device)?;
    let [hi, lo] = read_speed_kbs.to_be_bytes();
    let cdb = [0xBB, 0, hi, lo, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
    if sg_cmd(f.as_raw_fd(), &cdb) { Ok(()) } else { Err(std::io::Error::other("the drive refused SET CD-ROM SPEED")) }
}

/// Asks the drive to read at `times` x 1x (176 kB/s each). A drive reading at full speed and then waiting for a
/// slow consumer brakes and speeds up again; a moderate steady speed lets it run evenly. 0 changes nothing.
pub fn set_speed(device: &str, times: u32) -> std::io::Result<()> {
    if times == 0 {
        return Ok(());
    }
    set_cd_speed(device, (times.saturating_mul(KB_PER_SECOND_1X)).min(u32::from(u16::MAX) - 1) as u16)
}

/// Gives the drive its own speed back (the fastest it chooses itself).
pub fn reset_speed(device: &str) -> std::io::Result<()> {
    set_cd_speed(device, 0xFFFF)
}

/// What `eject` found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EjectOutcome {
    Ejected,
    /// The tray is already open or there is no disc: nothing was sent (a command could close the tray).
    NothingToEject,
    Failed,
}

/// Opens the tray of [`device`], but only if the kernel says a disc is in. Never closes a tray.
pub fn eject(device: &str) -> std::io::Result<EjectOutcome> {
    // The SCSI commands below pass the filter of the kernel only on a descriptor that is open for writing too (as for the speed);
    // a device that does not allow it is opened for reading, which still serves the kernel's own eject.
    let f = match OpenOptions::new().read(true).write(true).custom_flags(O_NONBLOCK).open(device) {
        Ok(f) => f,
        Err(_) => OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(device)?,
    };
    let fd = f.as_raw_fd();
    // SAFETY: plain ioctl with an integer argument on an open descriptor.
    let status = unsafe { libc::ioctl(fd, CDROM_DRIVE_STATUS as _, CDSL_CURRENT) };
    match status {
        CDS_DISC_OK => {}
        CDS_NO_DISC | CDS_TRAY_OPEN => return Ok(EjectOutcome::NothingToEject),
        // The drive cannot tell (-1) or reports something else (not ready): look at the table of contents.
        _ => {
            let has_disc = CdReader::open_path(device).and_then(|r| r.read_toc()).is_ok();
            if !has_disc {
                return Ok(EjectOutcome::NothingToEject);
            }
        }
    }
    // The kernel's way first. USB drives often refuse it; then SCSI does it:
    // PREVENT ALLOW MEDIUM REMOVAL (allow), then START STOP UNIT with LoEj=1, Start=0.
    // A door that was locked (by the reader of a rip that ended hard) is unlocked first; if that does not work, the eject tells.
    // SAFETY: plain ioctl with an integer argument on an open descriptor.
    unsafe { libc::ioctl(fd, CDROM_LOCKDOOR as _, 0) };
    // SAFETY: plain ioctl without argument on an open descriptor.
    if unsafe { libc::ioctl(fd, CDROMEJECT as _) } == 0 {
        return Ok(EjectOutcome::Ejected);
    }
    let ok = sg_cmd(fd, &[0x1E, 0, 0, 0, 0, 0]) && sg_cmd(fd, &[0x1B, 0, 0, 0, 0x02, 0]);
    Ok(if ok { EjectOutcome::Ejected } else { EjectOutcome::Failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sectors_per_track() {
        let info = TocInfo {
            tracks: vec![
                TocTrack { number: 1, start_lba: 0, is_audio: true },
                TocTrack { number: 2, start_lba: 1000, is_audio: true },
                TocTrack { number: 3, start_lba: 30_000, is_audio: false },
            ],
            leadout_lba: 40_000,
        };
        // Track 2 ends at the audio session's end: 30000 - 11400.
        assert_eq!(track_sectors(&info), vec![(1, 1000), (2, 30_000 - 11_400 - 1000)]);
    }

    #[test]
    fn io_errors_are_sorted() {
        let gone = CdReaderError::Io(std::io::Error::from_raw_os_error(libc::ENODEV));
        let bad = CdReaderError::Io(std::io::Error::from_raw_os_error(libc::EIO));
        assert_eq!(classify(&gone), ErrorClass::DriveGone);
        assert_eq!(classify(&bad), ErrorClass::Read);
        assert_eq!(classify(&CdReaderError::NoUsableDrive), ErrorClass::DriveGone);
        assert_eq!(classify(&CdReaderError::Parse("x".into())), ErrorClass::Read);
    }

    #[test]
    fn drives_are_found_by_their_sysfs_names_in_order() {
        let d = tempfile::TempDir::new().unwrap();
        for n in ["sr10", "sr0", "sr2", "sda", "sr", "srx", "loop0"] {
            std::fs::create_dir(d.path().join(n)).unwrap();
        }
        assert_eq!(devices_in(d.path()), ["/dev/sr0", "/dev/sr2", "/dev/sr10"]);
        assert!(devices_in(std::path::Path::new("/nonexistent")).is_empty());
    }

    #[test]
    fn a_speed_of_a_missing_device_is_an_error_and_zero_changes_nothing() {
        assert!(set_speed("/nonexistent/device", 8).is_err());
        assert!(reset_speed("/nonexistent/device").is_err());
        assert!(set_speed("/nonexistent/device", 0).is_ok(), "0 = the drive's own speed: no request, so no open");
    }

    #[test]
    fn the_status_of_a_missing_device_is_an_error() {
        assert!(status("/nonexistent/device").is_err());
    }

    #[test]
    fn eject_of_a_missing_device_is_an_error_not_a_command() {
        assert!(eject("/nonexistent/device").is_err());
    }
}
