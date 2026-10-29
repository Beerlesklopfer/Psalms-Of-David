// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Notices when an optical drive appears, disappears or gets another disc, from the kernel's
//! uevents (netlink `NETLINK_KOBJECT_UEVENT`, group 1). The same events udev itself works from.
//!
//! Why not libudev: libudev would add a shared library that must be present when the app is
//! built and when it runs (and a version to match), only to read properties such as
//! `ID_CDROM_MEDIA` that udev's rules derive from the very same kernel event by asking the drive.
//! The app asks the drive itself anyway (the table of contents decides whether a disc is audio),
//! so the kernel event is enough as a trigger and the socket needs nothing but libc. The limits:
//! the kernel group is readable only where the process may open a netlink socket (some sandboxes
//! forbid it; [`available`] says so), and it sees events of the initial network namespace only.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// `NETLINK_KOBJECT_UEVENT` (linux/netlink.h).
const NETLINK_KOBJECT_UEVENT: libc::c_int = 15;
/// The multicast group of kernel uevents.
const KERNEL_UEVENT_GROUP: u32 = 1;
/// Major device number of SCSI CD-ROM drives (`sr`, linux/major.h `SCSI_CDROM_MAJOR`).
pub const SCSI_CDROM_MAJOR: u32 = 11;
/// How long the receiving thread waits for a message before it looks at the stop flag.
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(500);

/// One kernel uevent: the action and its `KEY=value` properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uevent {
    pub action: String,
    pub props: HashMap<String, String>,
}

/// Parses a datagram of the kernel: `ACTION@DEVPATH\0KEY=value\0...`. Messages that are not in that
/// form (for instance from udev's own group, which starts with "libudev") give None.
pub fn parse(msg: &[u8]) -> Option<Uevent> {
    let mut parts = msg.split(|b| *b == 0);
    let head = std::str::from_utf8(parts.next()?).ok()?;
    let (action, _devpath) = head.split_once('@')?;
    if action.is_empty() || action.contains(' ') {
        return None;
    }
    let mut props = HashMap::new();
    for p in parts {
        if let Ok(s) = std::str::from_utf8(p)
            && let Some((k, v)) = s.split_once('=')
        {
            props.insert(k.to_owned(), v.to_owned());
        }
    }
    Some(Uevent { action: action.to_owned(), props })
}

/// Does this event mean "look at the drives again"? A disc drive (block device, whole disk, major
/// [`SCSI_CDROM_MAJOR`]) that is added or removed, or reports a medium change or an eject request.
pub fn relevant(e: &Uevent) -> bool {
    let p = |k: &str| e.props.get(k).map(String::as_str);
    if p("SUBSYSTEM") != Some("block") || p("DEVTYPE") != Some("disk") {
        return false;
    }
    if p("MAJOR").and_then(|m| m.parse::<u32>().ok()) != Some(SCSI_CDROM_MAJOR) {
        return false;
    }
    match e.action.as_str() {
        "add" | "remove" => true,
        "change" => p("DISK_MEDIA_CHANGE") == Some("1") || p("DISK_EJECT_REQUEST") == Some("1"),
        _ => false,
    }
}

/// What a source of messages delivers.
pub enum Received {
    Message(Vec<u8>),
    /// Nothing arrived within the timeout.
    Timeout,
    /// The source is gone; the watch ends.
    Closed,
}

/// The netlink socket.
struct Socket(libc::c_int);

impl Socket {
    fn open() -> io::Result<Socket> {
        // SAFETY: plain system calls; the descriptor is owned by the returned value.
        unsafe {
            let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let s = Socket(fd);
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            addr.nl_groups = KERNEL_UEVENT_GROUP;
            if libc::bind(fd, (&raw const addr).cast(), std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t) < 0 {
                return Err(io::Error::last_os_error());
            }
            let tv = libc::timeval { tv_sec: RECEIVE_TIMEOUT.as_secs() as libc::time_t, tv_usec: RECEIVE_TIMEOUT.subsec_micros() as libc::suseconds_t };
            if libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, (&raw const tv).cast(), std::mem::size_of::<libc::timeval>() as libc::socklen_t) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(s)
        }
    }

    fn receive(&self) -> Received {
        let mut buf = vec![0u8; 8192];
        // SAFETY: `buf` and `addr` are valid for the call; the lengths passed are theirs.
        unsafe {
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t;
            let n = libc::recvfrom(self.0, buf.as_mut_ptr().cast(), buf.len(), 0, (&raw mut addr).cast(), &mut len);
            if n < 0 {
                return match io::Error::last_os_error().raw_os_error() {
                    Some(libc::EAGAIN) | Some(libc::EINTR) | Some(libc::ENOBUFS) => Received::Timeout,
                    _ => Received::Closed,
                };
            }
            // Only the kernel (port id 0) may speak here; a process that sends to the group is ignored.
            if addr.nl_pid != 0 {
                return Received::Timeout;
            }
            buf.truncate(n as usize);
            Received::Message(buf)
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        // SAFETY: the descriptor is ours and closed once.
        unsafe { libc::close(self.0) };
    }
}

/// Can the kernel's uevents be listened to here? (Opens and closes a socket.)
pub fn available() -> bool {
    Socket::open().is_ok()
}

/// A running watch: a thread that counts relevant events. The owner asks [`Watch::take`] now and
/// then. Dropping it stops the thread.
pub struct Watch {
    count: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// Listens to the kernel's uevents. Fails where no netlink socket can be opened.
    pub fn start() -> io::Result<Watch> {
        let socket = Socket::open()?;
        Ok(Watch::spawn(move || socket.receive()))
    }

    /// Runs the watch over any source of messages (tests use a channel).
    pub fn spawn(mut source: impl FnMut() -> Received + Send + 'static) -> Watch {
        let count = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (c, s) = (count.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match source() {
                    Received::Message(m) => {
                        if parse(&m).is_some_and(|e| relevant(&e)) {
                            c.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Received::Timeout => {}
                    Received::Closed => break,
                }
            }
        });
        Watch { count, stop, thread: Some(thread) }
    }

    /// The number of relevant events since the last call.
    pub fn take(&self) -> u32 {
        self.count.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn msg(action: &str, props: &[(&str, &str)]) -> Vec<u8> {
        let mut v = format!("{action}@/devices/pci0000:00/block/sr1").into_bytes();
        for (k, val) in props {
            v.push(0);
            v.extend_from_slice(format!("{k}={val}").as_bytes());
        }
        v
    }

    const DISK: [(&str, &str); 3] = [("SUBSYSTEM", "block"), ("DEVTYPE", "disk"), ("MAJOR", "11")];

    fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        let mut p = DISK.to_vec();
        p.extend_from_slice(extra);
        p
    }

    #[test]
    fn parses_the_kernel_format() {
        let e = parse(&msg("change", &with(&[("DEVNAME", "sr1"), ("DISK_MEDIA_CHANGE", "1")]))).unwrap();
        assert_eq!(e.action, "change");
        assert_eq!(e.props["DEVNAME"], "sr1");
        assert!(parse(b"libudev\0\xfe\xed\xca\xfe").is_none(), "udev's own format is not ours");
        assert!(parse(b"").is_none());
    }

    #[test]
    fn a_new_disc_a_new_drive_and_a_removed_drive_matter() {
        let r = |action, extra: &[(&'static str, &'static str)]| relevant(&parse(&msg(action, &with(extra))).unwrap());
        assert!(r("change", &[("DISK_MEDIA_CHANGE", "1")]));
        assert!(r("change", &[("DISK_EJECT_REQUEST", "1")]));
        assert!(r("add", &[]));
        assert!(r("remove", &[]));
        assert!(!r("change", &[]), "a change that is no medium change");
        assert!(!r("bind", &[]));
    }

    #[test]
    fn other_devices_do_not_matter() {
        let p = |props: &[(&str, &str)]| relevant(&parse(&msg("add", props)).unwrap());
        assert!(!p(&[("SUBSYSTEM", "block"), ("DEVTYPE", "disk"), ("MAJOR", "8")]), "a hard disk");
        assert!(!p(&[("SUBSYSTEM", "block"), ("DEVTYPE", "partition"), ("MAJOR", "11")]));
        assert!(!p(&[("SUBSYSTEM", "usb"), ("DEVTYPE", "disk"), ("MAJOR", "11")]));
    }

    #[test]
    fn the_watch_counts_relevant_events_only_and_stops() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let rx = std::sync::Mutex::new(rx);
        let w = Watch::spawn(move || match rx.lock().unwrap().recv_timeout(Duration::from_millis(20)) {
            Ok(m) => Received::Message(m),
            Err(mpsc::RecvTimeoutError::Timeout) => Received::Timeout,
            Err(_) => Received::Closed,
        });
        tx.send(msg("change", &with(&[("DISK_MEDIA_CHANGE", "1")]))).unwrap();
        tx.send(msg("change", &with(&[]))).unwrap();
        tx.send(msg("add", &[("SUBSYSTEM", "net")])).unwrap();
        tx.send(msg("remove", &with(&[]))).unwrap();
        let mut got = 0;
        for _ in 0..100 {
            got += w.take();
            if got >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got, 2);
        assert_eq!(w.take(), 0, "taken events are not counted again");
        drop(w); // joins the thread
    }

    #[test]
    fn the_real_socket_opens_where_the_system_allows_it() {
        // Where netlink is forbidden (some sandboxes) `available` is false and `start` fails alike.
        let a = available();
        assert_eq!(Watch::start().is_ok(), a);
    }
}
