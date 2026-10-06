//! Per-user single instance and compositor-friendly activation on Linux.
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Show,
    Toggle,
    Hide,
    Quit,
}

impl Request {
    fn byte(self) -> u8 {
        match self {
            Self::Show => 1,
            Self::Toggle => 2,
            Self::Hide => 3,
            Self::Quit => 4,
        }
    }
    fn parse(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Show),
            2 => Some(Self::Toggle),
            3 => Some(Self::Hide),
            4 => Some(Self::Quit),
            _ => None,
        }
    }
}

pub enum Instance {
    Primary(Server),
    Forwarded,
}

pub struct Server {
    socket: UnixDatagram,
    path: PathBuf,
    _lock: File,
}

pub fn runtime_dir() -> io::Result<PathBuf> {
    // A private fallback also works for SSH/test sessions without XDG_RUNTIME_DIR.
    let uid = unsafe { libc::geteuid() };
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("quicker-rs-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let metadata = std::fs::symlink_metadata(&dir)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Quicker runtime directory must be owned by you with mode 0700",
        ));
    }
    Ok(dir)
}

pub fn start(dir: &Path, request: Request) -> io::Result<Instance> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("instance.lock"))?;
    let path = dir.join("control.sock");
    let result = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::WouldBlock {
            return Err(err);
        }
        let socket = UnixDatagram::unbound()?;
        // The primary may have acquired the lock but not bound its socket yet.
        for attempt in 0..50 {
            match socket.send_to(&[request.byte()], &path) {
                Ok(_) => return Ok(Instance::Forwarded),
                Err(err)
                    if attempt < 49
                        && matches!(
                            err.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                        ) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(err) => return Err(err),
            }
        }
        unreachable!();
    }
    // Only the lock holder can remove a socket left by a crashed process.
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let socket = UnixDatagram::bind(&path)?;
    socket.set_nonblocking(true)?;
    Ok(Instance::Primary(Server {
        socket,
        path,
        _lock: lock,
    }))
}

impl Server {
    pub fn receive(&self) -> io::Result<Option<Request>> {
        let mut bytes = [0; 16];
        match self.socket.recv(&mut bytes) {
            Ok(1) => Ok(Request::parse(bytes[0])),
            Ok(_) => Ok(None),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_instance_forwards_and_primary_can_restart() {
        let dir = tempfile::tempdir().unwrap();
        let Instance::Primary(server) = start(dir.path(), Request::Show).unwrap() else {
            panic!("not primary");
        };
        assert!(matches!(
            start(dir.path(), Request::Toggle).unwrap(),
            Instance::Forwarded
        ));
        assert_eq!(server.receive().unwrap(), Some(Request::Toggle));
        assert_eq!(server.receive().unwrap(), None);
        drop(server);
        assert!(matches!(
            start(dir.path(), Request::Show).unwrap(),
            Instance::Primary(_)
        ));
    }

    #[test]
    fn stale_socket_is_recovered_under_lock() {
        let dir = tempfile::tempdir().unwrap();
        drop(UnixDatagram::bind(dir.path().join("control.sock")).unwrap());
        assert!(matches!(
            start(dir.path(), Request::Show).unwrap(),
            Instance::Primary(_)
        ));
    }
}
