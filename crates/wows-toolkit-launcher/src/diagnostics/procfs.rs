//! What `/proc` tells about the game's threads and sockets.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::path::PathBuf;

use wgc_launch::ProcessId;

/// A Linux thread id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ThreadId(pub(super) u32);

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An x86_64 system call number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SyscallNr(u32);

/// A file descriptor number of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fd(u64);

/// The kernel's inode number of a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SocketInode(u64);

/// User plus system CPU time in clock ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CpuTicks(u64);

impl fmt::Display for CpuTicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `/proc/<pid>/task/<tid>/syscall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThreadSyscall {
    Running,
    /// Stopped outside a system call.
    User,
    Blocked {
        nr: SyscallNr,
        first_arg: u64,
    },
}

impl ThreadSyscall {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let mut fields = text.split_whitespace();
        let nr = fields.next()?;
        if nr == "running" {
            return Some(Self::Running);
        }
        if nr == "-1" {
            return Some(Self::User);
        }

        let nr = SyscallNr(nr.parse().ok()?);
        let first_arg = u64::from_str_radix(fields.next()?.strip_prefix("0x")?, 16).ok()?;

        Some(Self::Blocked { nr, first_arg })
    }

    /// The descriptor the call works on, for calls whose first argument is one.
    pub(super) fn fd(self) -> Option<Fd> {
        let Self::Blocked { nr, first_arg } = self else {
            return None;
        };

        syscall_info(nr).filter(|info| info.first_arg_is_fd).map(|_| Fd(first_arg))
    }

    pub(super) fn kind(self) -> SyscallKind {
        let Self::Blocked { nr, .. } = self else {
            return SyscallKind::Other;
        };

        syscall_info(nr).map_or(SyscallKind::Other, |info| info.kind)
    }
}

impl fmt::Display for ThreadSyscall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::User => write!(f, "user"),
            Self::Blocked { nr, .. } => match syscall_info(*nr) {
                Some(info) => write!(f, "{}", info.name),
                None => write!(f, "nr={}", nr.0),
            },
        }
    }
}

/// What a sampled system call is, for choosing the rows to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyscallKind {
    /// Socket calls and the waits a socket read or write blocks in.
    Network,
    /// Reads and writes that matter when their descriptor is a socket.
    FdIo,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SyscallInfo {
    pub(super) name: &'static str,
    pub(super) kind: SyscallKind,
    pub(super) first_arg_is_fd: bool,
}

/// The fields of `/proc/<pid>/task/<tid>/stat` the capture keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThreadStat {
    pub(super) comm: String,
    pub(super) state: char,
    pub(super) cpu: CpuTicks,
}

impl ThreadStat {
    pub(super) fn parse(text: &str) -> Option<Self> {
        // The command name may hold spaces and parentheses; it ends at the last `)`.
        let open = text.find('(')?;
        let close = text.rfind(')')?;
        let comm = text.get(open + 1..close)?.to_owned();
        let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();

        let state = rest.first()?.chars().next()?;
        // utime and stime, fields 14 and 15 of the line.
        let utime: u64 = rest.get(11)?.parse().ok()?;
        let stime: u64 = rest.get(12)?.parse().ok()?;

        Some(Self { comm, state, cpu: CpuTicks(utime + stime) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    fn name(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// The kernel's TCP states, which UDP sockets reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SocketState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    /// Closed for TCP; not connected for UDP.
    Unconnected,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    NewSynRecv,
}

impl SocketState {
    fn from_code(code: u8) -> Option<Self> {
        let state = match code {
            0x01 => Self::Established,
            0x02 => Self::SynSent,
            0x03 => Self::SynRecv,
            0x04 => Self::FinWait1,
            0x05 => Self::FinWait2,
            0x06 => Self::TimeWait,
            0x07 => Self::Unconnected,
            0x08 => Self::CloseWait,
            0x09 => Self::LastAck,
            0x0A => Self::Listen,
            0x0B => Self::Closing,
            0x0C => Self::NewSynRecv,
            _ => return None,
        };

        Some(state)
    }

    /// The names `ss` prints.
    fn name(self) -> &'static str {
        match self {
            Self::Established => "ESTAB",
            Self::SynSent => "SYN-SENT",
            Self::SynRecv => "SYN-RECV",
            Self::FinWait1 => "FIN-WAIT-1",
            Self::FinWait2 => "FIN-WAIT-2",
            Self::TimeWait => "TIME-WAIT",
            Self::Unconnected => "UNCONN",
            Self::CloseWait => "CLOSE-WAIT",
            Self::LastAck => "LAST-ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
            Self::NewSynRecv => "NEW-SYN-RECV",
        }
    }
}

/// One row of `/proc/<pid>/net/{tcp,tcp6,udp,udp6}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SocketEntry {
    pub(super) protocol: Protocol,
    pub(super) local: SocketAddr,
    pub(super) remote: SocketAddr,
    pub(super) state: SocketState,
    /// Bytes queued to send.
    pub(super) send_queue: u64,
    /// Bytes received and not yet read.
    pub(super) receive_queue: u64,
    pub(super) inode: SocketInode,
}

impl SocketEntry {
    /// A table row: `sl local remote st tx:rx tr:when retrnsmt uid timeout inode ...`.
    pub(super) fn parse(protocol: Protocol, line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split_whitespace().collect();

        let local = socket_addr(fields.get(1)?)?;
        let remote = socket_addr(fields.get(2)?)?;
        let state = SocketState::from_code(u8::from_str_radix(fields.get(3)?, 16).ok()?)?;
        let (send, receive) = fields.get(4)?.split_once(':')?;
        let inode = SocketInode(fields.get(9)?.parse().ok()?);

        Some(Self {
            protocol,
            local,
            remote,
            state,
            send_queue: u64::from_str_radix(send, 16).ok()?,
            receive_queue: u64::from_str_radix(receive, 16).ok()?,
            inode,
        })
    }
}

impl fmt::Display for SocketEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} send_q={} recv_q={}",
            self.protocol.name(),
            self.state.name(),
            self.local,
            self.remote,
            self.send_queue,
            self.receive_queue,
        )
    }
}

/// The name and kind of the x86_64 system calls a game thread is likely to sit in.
pub(super) fn syscall_info(nr: SyscallNr) -> Option<SyscallInfo> {
    let (name, kind, first_arg_is_fd) = match nr.0 {
        0 => ("read", SyscallKind::FdIo, true),
        1 => ("write", SyscallKind::FdIo, true),
        7 => ("poll", SyscallKind::Network, false),
        16 => ("ioctl", SyscallKind::Other, true),
        19 => ("readv", SyscallKind::FdIo, true),
        20 => ("writev", SyscallKind::FdIo, true),
        23 => ("select", SyscallKind::Network, false),
        24 => ("sched_yield", SyscallKind::Other, false),
        35 => ("nanosleep", SyscallKind::Other, false),
        41 => ("socket", SyscallKind::Network, false),
        42 => ("connect", SyscallKind::Network, true),
        43 => ("accept", SyscallKind::Network, true),
        44 => ("sendto", SyscallKind::Network, true),
        45 => ("recvfrom", SyscallKind::Network, true),
        46 => ("sendmsg", SyscallKind::Network, true),
        47 => ("recvmsg", SyscallKind::Network, true),
        48 => ("shutdown", SyscallKind::Network, true),
        49 => ("bind", SyscallKind::Network, true),
        50 => ("listen", SyscallKind::Network, true),
        51 => ("getsockname", SyscallKind::Network, true),
        52 => ("getpeername", SyscallKind::Network, true),
        53 => ("socketpair", SyscallKind::Network, false),
        54 => ("setsockopt", SyscallKind::Network, true),
        55 => ("getsockopt", SyscallKind::Network, true),
        202 => ("futex", SyscallKind::Other, false),
        219 => ("restart_syscall", SyscallKind::Other, false),
        230 => ("clock_nanosleep", SyscallKind::Other, false),
        232 => ("epoll_wait", SyscallKind::Network, true),
        270 => ("pselect6", SyscallKind::Network, false),
        271 => ("ppoll", SyscallKind::Network, false),
        281 => ("epoll_pwait", SyscallKind::Network, true),
        288 => ("accept4", SyscallKind::Network, true),
        299 => ("recvmmsg", SyscallKind::Network, true),
        307 => ("sendmmsg", SyscallKind::Network, true),
        441 => ("epoll_pwait2", SyscallKind::Network, true),
        _ => return None,
    };

    Some(SyscallInfo { name, kind, first_arg_is_fd })
}

/// `socket:[<inode>]`, the link of a socket descriptor.
pub(super) fn socket_inode(link: &str) -> Option<SocketInode> {
    let inode = link.strip_prefix("socket:[")?.strip_suffix(']')?;

    Some(SocketInode(inode.parse().ok()?))
}

/// `0100007F:1F90`: the address as the kernel's native-endian 32-bit words, then the port.
fn socket_addr(field: &str) -> Option<SocketAddr> {
    let (address, port) = field.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;

    let ip = match address.len() {
        8 => IpAddr::V4(Ipv4Addr::from(u32::from_str_radix(address, 16).ok()?.to_ne_bytes())),
        32 => {
            let mut octets = [0u8; 16];
            for (word, chunk) in octets.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let hex = address.get(word * 8..word * 8 + 8)?;
                *chunk = u32::from_str_radix(hex, 16).ok()?.to_ne_bytes();
            }
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };

    Some(SocketAddr::new(ip, port))
}

/// The sockets of `pid`'s network namespace, by inode.
pub(super) fn socket_table(pid: ProcessId) -> HashMap<SocketInode, SocketEntry> {
    const TABLES: [(&str, Protocol); 4] =
        [("tcp", Protocol::Tcp), ("tcp6", Protocol::Tcp), ("udp", Protocol::Udp), ("udp6", Protocol::Udp)];

    let mut table = HashMap::new();
    for (name, protocol) in TABLES {
        // A namespace without IPv6 has no tcp6 or udp6 table.
        let Ok(text) = std::fs::read_to_string(format!("/proc/{}/net/{name}", pid.0)) else {
            continue;
        };
        for entry in text.lines().skip(1).filter_map(|line| SocketEntry::parse(protocol, line)) {
            table.insert(entry.inode, entry);
        }
    }

    table
}

/// The inodes of `pid`'s socket descriptors.
pub(super) fn socket_inodes(pid: ProcessId) -> std::io::Result<Vec<SocketInode>> {
    let mut inodes = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/{}/fd", pid.0))?.flatten() {
        // Descriptors close between listing and reading.
        let Ok(link) = std::fs::read_link(entry.path()) else {
            continue;
        };
        if let Some(inode) = link.to_str().and_then(socket_inode) {
            inodes.push(inode);
        }
    }

    Ok(inodes)
}

/// The link of descriptor `fd` of `pid`.
pub(super) fn fd_target(pid: ProcessId, fd: Fd) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{}/fd/{}", pid.0, fd.0)).ok()
}

/// The threads of `pid`.
pub(super) fn threads(pid: ProcessId) -> std::io::Result<Vec<ThreadId>> {
    let mut threads = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/{}/task", pid.0))?.flatten() {
        if let Some(tid) = entry.file_name().to_str().and_then(|name| name.parse().ok()) {
            threads.push(ThreadId(tid));
        }
    }

    Ok(threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_syscall_lines_parse() {
        let parsed = [
            "running\n",
            "-1 0x7ffd5a0c8e10 0x7fb261c22332\n",
            "270 0x6 0x7ffd5a0c8e10 0x0 0x7ffd5a0c8f00 0x0 0x7ffd5a0c8e00 0x7ffd5a0c8d98 0x7fb261c22332\n",
            "45 0x1b2f 0x7f00 0x10000 0x0 0x0 0x0 0x7ffd5a0c8d98 0x7fb261c22332\n",
            "garbage\n",
        ]
        .map(|line| ThreadSyscall::parse(line).map(|syscall| format!("{syscall} fd={:?}", syscall.fd())));

        insta::assert_debug_snapshot!(parsed, @r#"
        [
            Some(
                "running fd=None",
            ),
            Some(
                "user fd=None",
            ),
            Some(
                "pselect6 fd=None",
            ),
            Some(
                "recvfrom fd=Some(Fd(6959))",
            ),
            None,
        ]
        "#);
    }

    #[test]
    fn thread_stat_keeps_a_command_name_with_parentheses() {
        let line = "769756 (Wor (ld) s) S 769000 769000 768400 0 -1 4194560 120 0 0 0 1234 56 0 0 20 0 97 0 1 2 3\n";

        insta::assert_debug_snapshot!(ThreadStat::parse(line), @r#"
        Some(
            ThreadStat {
                comm: "Wor (ld) s",
                state: 'S',
                cpu: CpuTicks(
                    1290,
                ),
            },
        )
        "#);
    }

    #[test]
    fn socket_table_rows_parse() {
        let rows = [
            (
                Protocol::Tcp,
                "  12: 4A01A8C0:AFBA 5E16DF5C:01BB 01 0000018D:00000000 01:00000014 00000000  1000        0 9123456 1 0000000000000000 20 4 30 10 -1",
            ),
            (
                Protocol::Udp,
                " 3051: 00000000:887E 00000000:0000 07 00000000:000003C0 00:00000000 00000000  1000        0 9123400 2 0000000000000000 0",
            ),
            (
                Protocol::Tcp,
                "   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 77 1 0000000000000000 100 0 0 10 0",
            ),
        ]
        .map(|(protocol, line)| SocketEntry::parse(protocol, line).map(|entry| format!("{entry} inode={}", entry.inode.0)));

        insta::assert_debug_snapshot!(rows, @r#"
        [
            Some(
                "tcp ESTAB 192.168.1.74:44986 92.223.22.94:443 send_q=397 recv_q=0 inode=9123456",
            ),
            Some(
                "udp UNCONN 0.0.0.0:34942 0.0.0.0:0 send_q=0 recv_q=960 inode=9123400",
            ),
            Some(
                "tcp LISTEN [::1]:8080 [::]:0 send_q=0 recv_q=0 inode=77",
            ),
        ]
        "#);
    }

    #[test]
    fn socket_links_name_their_inode() {
        let parsed = ["socket:[9123456]", "/dev/ntsync", "anon_inode:[eventpoll]", "socket:[]"].map(socket_inode);

        insta::assert_debug_snapshot!(parsed, @r"
        [
            Some(
                SocketInode(
                    9123456,
                ),
            ),
            None,
            None,
            None,
        ]
        ");
    }
}
