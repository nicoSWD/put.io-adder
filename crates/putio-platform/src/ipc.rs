//! Single-instance forwarding.
//!
//! When the user clicks a magnet link or opens a `.torrent` while put.io adder is already running,
//! Windows and Linux start a *new process* with the item on the command line. That process hands the
//! item to the running instance over a per-user local socket and exits. (macOS routes such requests
//! to the running app through Launch Services, but the same channel also works there.)
//!
//! The channel is a security boundary, so it is deliberately tiny:
//!
//! * the only thing that can be sent is **one text string** (a magnet link or file path), which the
//!   receiver validates with `putio_core::intake::Intake::parse` like any other OS input;
//! * a frame is a fixed magic, a length (capped at [`MAX_PAYLOAD`]) and UTF-8 bytes;
//! * every read has a deadline, so a stalled or hostile client cannot hang the server;
//! * on Unix the socket lives in a private (`0700`, owned by us, not a symlink) directory;
//! * there are no commands, no replies with data, and nothing is ever executed.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream};

/// Longest accepted payload: a magnet link is at most 8 KiB (`putio_core::magnet::MAX_LEN`) and a
/// path is far shorter.
pub const MAX_PAYLOAD: usize = 16 * 1024;
const MAGIC: &[u8; 6] = b"PUTIO\x01";
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
/// Time a peer gets to complete a frame (read) or acknowledge it.
const IO_DEADLINE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);
const ACCEPT_POLL: Duration = Duration::from_millis(50);
/// Connections handled at the same time. A stalled client occupies a slot for at most [`IO_DEADLINE`];
/// beyond this many, new connections are dropped instead of queued.
const MAX_CONNECTIONS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("payload is larger than {MAX_PAYLOAD} bytes")]
    TooLarge,
    #[error("not a put.io adder frame")]
    BadFrame,
    #[error("the peer did not finish in time")]
    Timeout,
    #[error("the running instance rejected the message")]
    Rejected,
    #[error("the socket directory is not private to this user: {0}")]
    InsecureDirectory(String),
    #[error("could not determine where to put the socket")]
    NoLocation,
    #[error(transparent)]
    Io(#[from] io::Error),
}

// ---- framing --------------------------------------------------------------------------------

/// `MAGIC | u32 big-endian length | payload`.
pub fn encode_frame(payload: &str) -> Result<Vec<u8>, IpcError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(IpcError::TooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| IpcError::TooLarge)?;
    let mut frame = Vec::with_capacity(MAGIC.len() + 4 + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload.as_bytes());
    Ok(frame)
}

/// Fill `buffer` from a possibly non-blocking reader, giving up at `deadline`.
fn read_exact_by<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<(), IpcError> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(buffer.get_mut(filled..).unwrap_or_default()) {
            Ok(0) => return Err(IpcError::BadFrame),
            Ok(n) => filled += n,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(IpcError::Timeout);
                }
                thread::sleep(POLL);
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Read one frame. The length is validated *before* any payload buffer is allocated.
pub fn read_frame<R: Read>(reader: &mut R, deadline: Instant) -> Result<String, IpcError> {
    let mut header = [0u8; 10];
    read_exact_by(reader, &mut header, deadline)?;
    let (magic, length) = header.split_at(MAGIC.len());
    if magic != MAGIC {
        return Err(IpcError::BadFrame);
    }
    let length = u32::from_be_bytes(length.try_into().map_err(|_| IpcError::BadFrame)?) as usize;
    if length > MAX_PAYLOAD {
        return Err(IpcError::TooLarge);
    }
    let mut payload = vec![0u8; length];
    read_exact_by(reader, &mut payload, deadline)?;
    String::from_utf8(payload).map_err(|_| IpcError::BadFrame)
}

// ---- where the socket lives -----------------------------------------------------------------

/// Where this instance's socket or pipe is.
#[derive(Debug, Clone)]
pub enum Endpoint {
    /// A socket file inside a directory that must be private to the user (Unix).
    Path(PathBuf),
    /// A named pipe (Windows), made unique per user.
    Pipe(String),
}

impl Endpoint {
    /// The endpoint for this OS and user.
    pub fn for_current_user() -> Result<Self, IpcError> {
        let os = crate::paths::Os::current();
        if os == crate::paths::Os::Windows {
            let user: String = std::env::var("USERNAME")
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .take(64)
                .collect();
            return Ok(Self::Pipe(format!("putio-adder-{user}")));
        }
        let dir = crate::paths::runtime_dir(os, &crate::paths::process_env)
            .ok_or(IpcError::NoLocation)?;
        Ok(Self::Path(dir.join("ipc.sock")))
    }

    fn name(&self) -> io::Result<interprocess::local_socket::Name<'_>> {
        use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ToFsName, ToNsName};
        match self {
            Self::Path(path) => path.as_os_str().to_fs_name::<GenericFilePath>(),
            Self::Pipe(name) => name.as_str().to_ns_name::<GenericNamespaced>(),
        }
    }

    fn socket_dir(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => path.parent(),
            Self::Pipe(_) => None,
        }
    }
}

/// Create `dir` (mode `0700`) if needed and refuse to use it unless it is a real directory, owned by
/// us, and not accessible to group or others.
#[cfg(unix)]
fn ensure_private_dir(dir: &Path) -> Result<(), IpcError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir)?;

    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(IpcError::InsecureDirectory(
            "not a directory (or a symlink)".into(),
        ));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(IpcError::InsecureDirectory(format!(
            "mode is {:o}",
            meta.permissions().mode() & 0o777
        )));
    }
    // A file we create is owned by us; the directory must have the same owner.
    let probe = dir.join(format!(".probe-{}", std::process::id()));
    std::fs::write(&probe, b"")?;
    let probe_uid = std::fs::metadata(&probe).map(|m| m.uid());
    let _ = std::fs::remove_file(&probe);
    if probe_uid? != meta.uid() {
        return Err(IpcError::InsecureDirectory("owned by another user".into()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_dir(_dir: &Path) -> Result<(), IpcError> {
    Ok(())
}

// ---- client ---------------------------------------------------------------------------------

/// Send `messages` to the running instance, one frame each. An empty slice sends a single empty
/// frame, which means "bring your window to the front".
pub fn forward(endpoint: &Endpoint, messages: &[String]) -> Result<(), IpcError> {
    let empty = [String::new()];
    for message in if messages.is_empty() {
        &empty[..]
    } else {
        messages
    } {
        send_one(endpoint, message)?;
    }
    Ok(())
}

fn send_one(endpoint: &Endpoint, message: &str) -> Result<(), IpcError> {
    let frame = encode_frame(message)?;
    let mut stream = Stream::connect(endpoint.name()?)?;
    stream.write_all(&frame)?;
    stream.flush()?;
    stream.set_nonblocking(true)?;
    let mut ack = [0u8; 1];
    read_exact_by(&mut stream, &mut ack, Instant::now() + IO_DEADLINE)?;
    if ack == [ACK] {
        Ok(())
    } else {
        Err(IpcError::Rejected)
    }
}

// ---- server ---------------------------------------------------------------------------------

/// The listening side. Dropping it stops the thread and removes the socket file.
#[derive(Debug)]
pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    socket_file: Option<PathBuf>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(path) = &self.socket_file {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Debug)]
pub enum Instance {
    /// We are the first instance: messages from later launches arrive on the channel.
    Primary(Server),
    /// Another instance is running and has received our messages: exit.
    Forwarded,
}

/// Become the primary instance, or hand `messages` to the one that already is.
///
/// `on_message` runs after each received message (use it to wake the UI).
pub fn start_or_forward(
    endpoint: &Endpoint,
    messages: &[String],
    tx: Sender<String>,
    on_message: impl Fn() + Send + Sync + 'static,
) -> Result<Instance, IpcError> {
    // Two launches at the same moment can both see "nobody is listening"; the loser's bind fails
    // with AddrInUse, and the next round of this loop finds the winner.
    let mut attempts = 0;
    loop {
        attempts += 1;
        match forward(endpoint, messages) {
            Ok(()) => return Ok(Instance::Forwarded),
            // The running instance is there but unhappy with the message: do not start a second one.
            Err(error @ (IpcError::Rejected | IpcError::TooLarge)) => return Err(error),
            Err(_) => {}
        }
        if let Some(dir) = endpoint.socket_dir() {
            ensure_private_dir(dir)?;
        }
        match bind(endpoint) {
            Ok(listener) => {
                return Ok(Instance::Primary(serve(
                    endpoint, listener, tx, on_message,
                )?));
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempts < 5 => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn bind(endpoint: &Endpoint) -> io::Result<interprocess::local_socket::Listener> {
    // `try_overwrite` removes a stale socket file left by a crashed instance (we only get here
    // after a failed connect, so nobody is listening).
    ListenerOptions::new()
        .name(endpoint.name()?)
        .try_overwrite(true)
        .create_sync()
}

fn serve(
    endpoint: &Endpoint,
    listener: interprocess::local_socket::Listener,
    tx: Sender<String>,
    on_message: impl Fn() + Send + Sync + 'static,
) -> Result<Server, IpcError> {
    listener.set_nonblocking(ListenerNonblockingMode::Accept)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let on_message: Arc<dyn Fn() + Send + Sync> = Arc::new(on_message);
    let active = Arc::new(AtomicUsize::new(0));
    let thread = thread::Builder::new()
        .name("putio-ipc".into())
        .spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok(stream) => {
                        let slot = Slot(Arc::clone(&active));
                        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                            continue; // `slot` drops and gives the count back; the client sees an error
                        }
                        let (tx, on_message) = (tx.clone(), Arc::clone(&on_message));
                        // If the thread cannot be started the closure is dropped, which frees the slot.
                        let _ =
                            thread::Builder::new()
                                .name("putio-ipc-conn".into())
                                .spawn(move || {
                                    let _slot = slot;
                                    if handle(stream, &tx).is_ok() {
                                        on_message();
                                    }
                                });
                    }
                    Err(_) => thread::sleep(ACCEPT_POLL),
                }
            }
        })?;
    let socket_file = match endpoint {
        Endpoint::Path(path) => Some(path.clone()),
        Endpoint::Pipe(_) => None,
    };
    Ok(Server {
        stop,
        thread: Some(thread),
        socket_file,
    })
}

/// Holds one of the [`MAX_CONNECTIONS`] slots; released on drop, whatever happens to the thread.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One connection: read a frame within the deadline, pass it on, acknowledge.
fn handle(mut stream: Stream, tx: &Sender<String>) -> Result<(), IpcError> {
    stream.set_nonblocking(true)?;
    let outcome = read_frame(&mut stream, Instant::now() + IO_DEADLINE);
    let reply = if outcome.is_ok() { ACK } else { NAK };
    stream.set_nonblocking(false)?;
    // A peer that already left is not our problem.
    let _ = stream.write_all(&[reply]);
    let _ = stream.flush();
    match outcome {
        Ok(message) => tx.send(message).map_err(|_| IpcError::Rejected),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::mpsc;

    use super::*;

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    // ---- framing ----

    #[test]
    fn frames_round_trip() {
        for text in [
            "",
            "magnet:?xt=urn:btih:abc",
            "/tmp/ü.torrent",
            &"x".repeat(MAX_PAYLOAD),
        ] {
            let frame = encode_frame(text).unwrap();
            assert_eq!(read_frame(&mut Cursor::new(frame), far()).unwrap(), text);
        }
    }

    #[test]
    fn oversized_payloads_are_refused_on_both_sides() {
        assert!(matches!(
            encode_frame(&"x".repeat(MAX_PAYLOAD + 1)),
            Err(IpcError::TooLarge)
        ));
        // A peer that *claims* a huge length is rejected before anything is allocated or read.
        let mut hostile = Vec::from(&MAGIC[..]);
        hostile.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            read_frame(&mut Cursor::new(hostile), far()),
            Err(IpcError::TooLarge)
        ));
    }

    #[test]
    fn bad_magic_truncated_and_non_utf8_frames_are_rejected() {
        let mut bad_magic = encode_frame("hello").unwrap();
        bad_magic[0] = b'X';
        assert!(matches!(
            read_frame(&mut Cursor::new(bad_magic), far()),
            Err(IpcError::BadFrame)
        ));

        let mut truncated = encode_frame("hello world").unwrap();
        truncated.truncate(truncated.len() - 3);
        assert!(matches!(
            read_frame(&mut Cursor::new(truncated), far()),
            Err(IpcError::BadFrame)
        ));

        let mut not_utf8 = Vec::from(&MAGIC[..]);
        not_utf8.extend_from_slice(&2u32.to_be_bytes());
        not_utf8.extend_from_slice(&[0xff, 0xfe]);
        assert!(matches!(
            read_frame(&mut Cursor::new(not_utf8), far()),
            Err(IpcError::BadFrame)
        ));

        assert!(matches!(
            read_frame(&mut Cursor::new(Vec::new()), far()),
            Err(IpcError::BadFrame)
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new(b"GET / HTTP/1.1\r\n\r\n".to_vec()), far()),
            Err(IpcError::BadFrame)
        ));
    }

    /// Yields one byte per call, with a `WouldBlock` in between.
    struct Dribble {
        data: Vec<u8>,
        pos: usize,
        block_next: bool,
    }

    impl Read for Dribble {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.block_next {
                self.block_next = false;
                return Err(io::ErrorKind::WouldBlock.into());
            }
            self.block_next = true;
            match self.data.get(self.pos) {
                Some(&byte) if !buf.is_empty() => {
                    buf[0] = byte;
                    self.pos += 1;
                    Ok(1)
                }
                _ => Ok(0),
            }
        }
    }

    #[test]
    fn slow_but_progressing_peers_are_served() {
        let mut reader = Dribble {
            data: encode_frame("magnet:?x").unwrap(),
            pos: 0,
            block_next: false,
        };
        assert_eq!(read_frame(&mut reader, far()).unwrap(), "magnet:?x");
    }

    struct Stalled;
    impl Read for Stalled {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::WouldBlock.into())
        }
    }

    #[test]
    fn a_stalled_peer_times_out() {
        let started = Instant::now();
        let result = read_frame(&mut Stalled, Instant::now() + Duration::from_millis(100));
        assert!(matches!(result, Err(IpcError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    // ---- sockets (Unix hosts; the same code runs over named pipes on Windows) ----

    #[cfg(unix)]
    mod sockets {
        use std::os::unix::fs::PermissionsExt;

        use super::*;
        use crate::settings_store::test_dir::TempDir;

        /// Keep paths short: Unix socket paths are limited to ~100 bytes.
        fn short_dir(label: &str) -> TempDir {
            let n = std::process::id();
            let dir = PathBuf::from(format!("/tmp/pa-{label}-{n}"));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }

        fn endpoint(dir: &TempDir) -> Endpoint {
            Endpoint::Path(dir.0.join("ipc.sock"))
        }

        fn primary(endpoint: &Endpoint) -> (Server, mpsc::Receiver<String>) {
            let (tx, rx) = mpsc::channel();
            match start_or_forward(endpoint, &[], tx, || {}).unwrap() {
                Instance::Primary(server) => (server, rx),
                Instance::Forwarded => panic!("expected to become the primary instance"),
            }
        }

        #[test]
        fn a_second_launch_forwards_its_messages_and_exits() {
            let dir = short_dir("fwd");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);

            let (tx2, _rx2) = mpsc::channel();
            let second = start_or_forward(
                &endpoint,
                &["magnet:?xt=urn:btih:abc".into(), "/tmp/a.torrent".into()],
                tx2,
                || {},
            )
            .unwrap();
            assert!(matches!(second, Instance::Forwarded));
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "magnet:?xt=urn:btih:abc"
            );
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "/tmp/a.torrent"
            );
        }

        #[test]
        fn a_relaunch_without_arguments_sends_an_empty_focus_message() {
            let dir = short_dir("focus");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);
            let (tx2, _rx2) = mpsc::channel();
            assert!(matches!(
                start_or_forward(&endpoint, &[], tx2, || {}).unwrap(),
                Instance::Forwarded
            ));
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "");
        }

        #[test]
        fn on_message_wakes_the_ui() {
            let dir = short_dir("wake");
            let endpoint = endpoint(&dir);
            let (tx, _rx) = mpsc::channel();
            let woke = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&woke);
            let Instance::Primary(_server) = start_or_forward(&endpoint, &[], tx, move || {
                flag.store(true, Ordering::SeqCst);
            })
            .unwrap() else {
                panic!("primary expected")
            };
            forward(&endpoint, &["x".into()]).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !woke.load(Ordering::SeqCst) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(woke.load(Ordering::SeqCst));
        }

        #[test]
        fn dropping_the_server_frees_the_socket_for_the_next_instance() {
            let dir = short_dir("drop");
            let endpoint = endpoint(&dir);
            let (server, _rx) = primary(&endpoint);
            drop(server);
            assert!(!dir.0.join("ipc.sock").exists());
            let (_again, _rx) = primary(&endpoint);
        }

        #[test]
        fn a_stale_socket_file_from_a_crash_is_replaced() {
            let dir = short_dir("stale");
            let endpoint = endpoint(&dir);
            {
                let (server, _rx) = primary(&endpoint);
                // Simulate a crash: leak the socket file without stopping cleanly.
                std::mem::forget(server);
            }
            // The listener thread is still alive in this process, so kill that by replacing the
            // file with a dead socket: bind a plain UnixListener and drop it (file stays, nobody listens).
            let _ = std::fs::remove_file(dir.0.join("ipc.sock"));
            drop(std::os::unix::net::UnixListener::bind(dir.0.join("ipc.sock")).unwrap());
            assert!(dir.0.join("ipc.sock").exists());
            let (_server, rx) = primary(&endpoint);
            forward(&endpoint, &["after-crash".into()]).unwrap();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "after-crash"
            );
        }

        #[test]
        fn garbage_and_stalled_clients_do_not_block_real_ones() {
            let dir = short_dir("evil");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);

            // A client that connects and says nothing.
            let _silent = Stream::connect(endpoint.name().unwrap()).unwrap();
            // A client that sends a partial header and then stalls.
            let mut partial = Stream::connect(endpoint.name().unwrap()).unwrap();
            partial.write_all(&MAGIC[..3]).unwrap();
            // A client that sends garbage.
            let mut garbage = Stream::connect(endpoint.name().unwrap()).unwrap();
            garbage
                .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();

            // A legitimate client still gets through once the bad ones time out or are rejected.
            let started = Instant::now();
            forward(&endpoint, &["legit".into()]).unwrap();
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "legit");
            assert!(
                started.elapsed() < Duration::from_secs(8),
                "took {:?}",
                started.elapsed()
            );
            // …and none of the bad clients produced a message.
            assert!(rx.try_recv().is_err());
        }

        #[test]
        fn an_oversized_frame_is_rejected_with_a_nak() {
            let dir = short_dir("big");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);
            let mut client = Stream::connect(endpoint.name().unwrap()).unwrap();
            let mut hostile = Vec::from(&MAGIC[..]);
            hostile.extend_from_slice(&u32::MAX.to_be_bytes());
            client.write_all(&hostile).unwrap();
            client.set_nonblocking(true).unwrap();
            let mut ack = [0u8; 1];
            read_exact_by(
                &mut client,
                &mut ack,
                Instant::now() + Duration::from_secs(3),
            )
            .unwrap();
            assert_eq!(ack, [NAK]);
            assert!(rx.try_recv().is_err());
        }

        #[test]
        fn the_client_refuses_to_send_an_oversized_message() {
            let dir = short_dir("bigclient");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);
            assert!(matches!(
                forward(&endpoint, &["x".repeat(MAX_PAYLOAD + 1)]),
                Err(IpcError::TooLarge)
            ));
            assert!(rx.try_recv().is_err());
        }

        #[test]
        fn the_socket_directory_is_created_private() {
            let dir = short_dir("perm");
            let endpoint = endpoint(&dir);
            let (_server, _rx) = primary(&endpoint);
            assert_eq!(
                std::fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        #[test]
        fn a_group_or_world_accessible_directory_is_refused() {
            let dir = short_dir("open");
            std::fs::create_dir_all(&dir.0).unwrap();
            std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o777)).unwrap();
            let (tx, _rx) = mpsc::channel();
            let error = start_or_forward(&endpoint(&dir), &[], tx, || {}).unwrap_err();
            assert!(matches!(error, IpcError::InsecureDirectory(_)), "{error:?}");
        }

        #[test]
        fn a_symlinked_directory_is_refused() {
            let dir = short_dir("link");
            let target = short_dir("linktarget");
            std::fs::create_dir_all(&target.0).unwrap();
            std::fs::set_permissions(&target.0, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::os::unix::fs::symlink(&target.0, &dir.0).unwrap();
            let (tx, _rx) = mpsc::channel();
            let error = start_or_forward(&endpoint(&dir), &[], tx, || {}).unwrap_err();
            assert!(matches!(error, IpcError::InsecureDirectory(_)), "{error:?}");
            let _ = std::fs::remove_file(&dir.0);
        }
    }
}
