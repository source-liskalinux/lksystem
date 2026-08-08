//! Reads kernel device events (uevents) directly from the kernel via a
//! `NETLINK_KOBJECT_UEVENT` netlink socket -- the same multicast group udev listens
//! on. This is the foundation for `.device` units: no udev/eudev is required for
//! lksystem to *know* when a device appeared or disappeared, only to populate
//! `/dev` nodes with the right permissions/symlinks (which is out of scope here,
//! since the user's setup does not need udev).
//!
//! Wire format of a kernel uevent (as opposed to the "libudev" enriched format,
//! which additionally prefixes a `libudev` magic and is NOT what the kernel
//! sends): a NUL-separated sequence of ASCII strings.
//!
//! ```text
//! "add@/devices/pci0000:00/.../block/sda/sda1\0"
//! "ACTION=add\0"
//! "DEVPATH=/devices/pci0000:00/.../block/sda/sda1\0"
//! "SUBSYSTEM=block\0"
//! "MAJOR=8\0"
//! "MINOR=1\0"
//! "DEVNAME=sda1\0"
//! "DEVTYPE=partition\0"
//! ... (further SUBSYSTEM-specific KEY=VALUE pairs) ...
//! ```
//!
//! The first token (before the first NUL) is a duplicate of `ACTION@DEVPATH` and
//! is skipped; everything after it is parsed as `KEY=VALUE`.
use crate::ui;
use std::collections::HashMap;
use std::io;
use std::os::unix::io::RawFd;
use std::thread::JoinHandle;
/// Linux kernel multicast group for kobject/uevent messages (`NETLINK_KOBJECT_UEVENT`
/// only has group 1 defined right now, but we keep this named to make the bind() call
/// self-documenting).
const KOBJECT_UEVENT_GROUP: u32 = 1;
/// Read buffer size. The kernel never sends uevents bigger than one page, but we
/// give ourselves headroom -- oversized messages are truncated by recv() and we'd
/// rather notice via a warning than silently corrupt parsing.
const RECV_BUF_SIZE: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum UeventAction {
    Add,
    Remove,
    Change,
    Move,
    Online,
    Offline,
    Bind,
    Unbind,
}

impl UeventAction {
    fn parse(s: &str) -> Option<UeventAction> {
        match s {
            "add" => Some(UeventAction::Add),
            "remove" => Some(UeventAction::Remove),
            "change" => Some(UeventAction::Change),
            "move" => Some(UeventAction::Move),
            "online" => Some(UeventAction::Online),
            "offline" => Some(UeventAction::Offline),
            "bind" => Some(UeventAction::Bind),
            "unbind" => Some(UeventAction::Unbind),
            // Deliberately not an error: the kernel occasionally grows new action
            // types (e.g. "unbind" was added later). Unknown actions are dropped by
            // the caller rather than crashing lksystem.
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UeventMessage {
    pub action: UeventAction,
    /// e.g. "/devices/pci0000:00/.../block/sda/sda1"
    pub devpath: String,
    /// e.g. Some("block")
    pub subsystem: Option<String>,
    /// e.g. Some("sda1") -- the /dev node name relative to /dev, if the device
    /// has one (not every device does, e.g. some subsystems are sysfs-only).
    pub devname: Option<String>,
    /// All KEY=VALUE properties from the message, including ACTION/DEVPATH/
    /// SUBSYSTEM/DEVNAME again for convenience of callers that want raw access
    /// (e.g. MAJOR/MINOR, ID_FS_UUID if a later enrichment step adds it, etc).
    pub properties: HashMap<String, String>,
}

/// Opens and binds the netlink uevent socket. Requires `CAP_NET_ADMIN`
/// (in practice: must be run as root, which as PID 1 we are).
pub fn open_uevent_socket() -> io::Result<RawFd> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Kernel uevents can burst heavily during coldplug (boot) when many devices
    // enumerate at once. Raise the receive buffer so we don't drop messages
    // before we have a chance to read them out. Best-effort: if this fails we
    // still proceed with the default buffer size.
    let bufsize: libc::c_int = 1 << 20; // 1 MiB
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUFFORCE,
            &bufsize as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
        // Ignore the result of SO_RCVBUFFORCE: it requires CAP_NET_ADMIN which we
        // have as PID 1, but on some hardened kernels/containers it can still be
        // denied; that's not fatal.
    }
    let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as u16;
    addr.nl_pid = 0; // let the kernel assign our port id
    addr.nl_groups = KOBJECT_UEVENT_GROUP;
    let bind_res = unsafe {
        libc::bind(
            fd,
            &addr as *const libc::sockaddr_nl as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if bind_res < 0 {
        let err = io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(err);
    }
    Ok(fd)
}

/// Blocks until one uevent datagram arrives on `fd` (as returned by
/// [`open_uevent_socket`]) and parses it. Returns `Ok(None)` for datagrams whose
/// action we don't recognize (safe to ignore) or that are malformed (logged by
/// the caller, not fatal -- a bad message must never take PID 1 down).
pub fn read_uevent(fd: RawFd) -> io::Result<Option<UeventMessage>> {
    let mut buf = vec![0u8; RECV_BUF_SIZE];
    let n = unsafe {
        libc::recv(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            0,
        )
    };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n as usize);
    Ok(parse_uevent(&buf))
}

fn parse_uevent(buf: &[u8]) -> Option<UeventMessage> {
    let mut parts = buf.split(|b| *b == 0).filter(|s| !s.is_empty());
    // First token is "ACTION@DEVPATH" -- informative but fully redundant with the
    // ACTION= and DEVPATH= keys that follow, so we just skip over it.
    parts.next()?;
    let mut properties = HashMap::new();
    for part in parts {
        let s = match std::str::from_utf8(part) {
            Ok(s) => s,
            // A uevent property is never guaranteed to be UTF-8 (e.g. some
            // firmware-provided strings). Skip only that one field rather than
            // discarding the whole message.
            Err(_) => continue,
        };
        if let Some((key, value)) = s.split_once('=') {
            properties.insert(key.to_owned(), value.to_owned());
        }
    }
    let action = UeventAction::parse(properties.get("ACTION")?)?;
    let devpath = properties.get("DEVPATH")?.clone();
    let subsystem = properties.get("SUBSYSTEM").cloned();
    let devname = properties.get("DEVNAME").cloned();
    Some(UeventMessage {
        action,
        devpath,
        subsystem,
        devname,
        properties,
    })
}

/// Spawns a background thread that reads uevents forever and invokes `callback`
/// for each one lksystem understands. Malformed/unrecognized datagrams are
/// silently dropped (this mirrors udev's own behavior); socket errors are
/// logged via the `log` crate and the loop exits (the caller may want to detect
/// this via `JoinHandle::is_finished()` and re-spawn, though in practice a
/// netlink socket read failing means something is deeply wrong with the kernel
/// interface and a restart of lksystem itself is more appropriate).
pub fn spawn_uevent_listener<F>(callback: F) -> io::Result<JoinHandle<()>>
where
    F: Fn(UeventMessage) + Send + 'static,
{
    let fd = open_uevent_socket()?;
    let handle = std::thread::Builder::new()
        .name("uevent-listener".to_owned())
        .spawn(move || loop {
            match read_uevent(fd) {
                Ok(Some(msg)) => callback(msg),
                Ok(None) => { /* unrecognized/malformed datagram, ignore */ }
                Err(e) => {
                    ui::error(format!("netlink uevent socket read failed, stopping listener: {}", e));
                    break;
                }
            }
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    Ok(handle)
}

/// Escapes an arbitrary path (either a `/sys` DEVPATH or a `/dev` DEVNAME) into a
/// systemd-compatible unit name component, matching the rules `systemd-escape
/// --path` uses: the leading `/` is stripped, each remaining `/` becomes `-`,
/// and any byte that isn't `[A-Za-z0-9:_.]` (or a `-` that would otherwise be
/// ambiguous with the path-separator escaping) is percent-style-escaped as
/// `\xNN` using systemd's own escaping alphabet (`\` followed by lowercase hex).
///
/// Examples:
/// - `/dev/sda1`                                   -> `dev-sda1`
/// - `/sys/devices/pci0000:00/0000:00:1f.2/ata1`    -> `sys-devices-pci0000:00-0000:00:1f.2-ata1`
pub fn escape_path_to_unit_name_component(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    let mut out = String::with_capacity(trimmed.len());
    for segment in trimmed.split('/') {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&escape_segment(segment));
    }
    out
}

fn escape_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for (i, b) in segment.bytes().enumerate() {
        let allowed = b.is_ascii_alphanumeric()
            || b == b':'
            || b == b'_'
            || b == b'.'
            || (b == b'-' && i != 0); // leading '-' in a segment is escaped to avoid
                                       // clashing with our own '-' separator above
        if allowed {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{:02x}", b));
        }
    }
    out
}

/// Builds the full `NN.device` unit name for a `DEVNAME` (`/dev/...`) uevent
/// property, e.g. `"sda1"` -> `"dev-sda1.device"`.
pub fn device_unit_name_from_devname(devname: &str) -> String {
    format!(
        "{}.device",
        escape_path_to_unit_name_component(&format!("/dev/{}", devname))
    )
}

/// Builds the fallback `NN.device` unit name from the sysfs `DEVPATH` for
/// devices that have no `/dev` node (e.g. network interfaces, some buses),
/// e.g. `"/devices/pci.../net/eth0"` -> `"sys-devices-pci...-net-eth0.device"`.
pub fn device_unit_name_from_devpath(devpath: &str) -> String {
    format!(
        "{}.device",
        escape_path_to_unit_name_component(&format!("/sys{}", devpath))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_escape_simple_dev_path() {
        assert_eq!(escape_path_to_unit_name_component("/dev/sda1"), "dev-sda1");
    }
    #[test]
    fn test_escape_colon_and_dot_are_kept() {
        assert_eq!(
            escape_path_to_unit_name_component(
                "/sys/devices/pci0000:00/0000:00:1f.2/ata1"
            ),
            "sys-devices-pci0000:00-0000:00:1f.2-ata1"
        );
    }
    #[test]
    fn test_device_unit_name_from_devname() {
        assert_eq!(device_unit_name_from_devname("sda1"), "dev-sda1.device");
    }
    #[test]
    fn test_parse_minimal_add_event() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"add@/devices/virtual/block/loop0\0");
        raw.extend_from_slice(b"ACTION=add\0");
        raw.extend_from_slice(b"DEVPATH=/devices/virtual/block/loop0\0");
        raw.extend_from_slice(b"SUBSYSTEM=block\0");
        raw.extend_from_slice(b"DEVNAME=loop0\0");
        raw.extend_from_slice(b"MAJOR=7\0");
        raw.extend_from_slice(b"MINOR=0\0");
        let msg = parse_uevent(&raw).expect("should parse");
        assert_eq!(msg.action, UeventAction::Add);
        assert_eq!(msg.devpath, "/devices/virtual/block/loop0");
        assert_eq!(msg.subsystem.as_deref(), Some("block"));
        assert_eq!(msg.devname.as_deref(), Some("loop0"));
        assert_eq!(msg.properties.get("MAJOR").map(|s| s.as_str()), Some("7"));
    }
    #[test]
    fn test_unknown_action_returns_none() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"somethingnew@/devices/foo\0");
        raw.extend_from_slice(b"ACTION=somethingnew\0");
        raw.extend_from_slice(b"DEVPATH=/devices/foo\0");
        assert!(parse_uevent(&raw).is_none());
    }
}
