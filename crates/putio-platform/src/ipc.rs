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
/// How long a launch waits for another launch to finish deciding who is the primary instance.
const LOCK_WAIT: Duration = Duration::from_secs(5);

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

/// Makes probe-file names unique within a process (several launches can be tested in one).
#[cfg(unix)]
static PROBE_SERIAL: AtomicUsize = AtomicUsize::new(0);

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
    let probe = dir.join(format!(
        ".probe-{}-{}",
        std::process::id(),
        PROBE_SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&probe, b"")?;
    let probe_uid = std::fs::metadata(&probe).map(|m| m.uid());
    let _ = std::fs::remove_file(&probe);
    if probe_uid? != meta.uid() {
        return Err(IpcError::InsecureDirectory("owned by another user".into()));
    }
    Ok(())
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // the same signature as the Unix version
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

/// Serialises start-up between launches (Unix): "is anyone listening? if not, become the listener"
/// must not interleave between two processes, or a second launch could delete the first one's
/// freshly bound socket. Held only during that decision, never while running.
struct StartupLock(#[allow(dead_code)] Option<std::fs::File>);

impl StartupLock {
    fn acquire(endpoint: &Endpoint) -> Result<Self, IpcError> {
        let Endpoint::Path(socket) = endpoint else {
            return Ok(Self(None)); // named pipes cannot go stale; creation is atomic
        };
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(socket.with_file_name("ipc.lock"))?;
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self(Some(file))),
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(IpcError::Timeout),
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
}

/// Become the primary instance, or hand `messages` to the one that already is.
///
/// A running instance is **never replaced**: the socket is only taken over when connecting to it was
/// *refused* (a file left behind by a crash) or it does not exist. If an instance exists but is slow or
/// unhappy, this returns an error instead of starting a second one.
///
/// `on_message` runs after each received message (use it to wake the UI).
pub fn start_or_forward(
    endpoint: &Endpoint,
    messages: &[String],
    tx: Sender<String>,
    on_message: impl Fn() + Send + Sync + 'static,
) -> Result<Instance, IpcError> {
    if let Some(dir) = endpoint.socket_dir() {
        ensure_private_dir(dir)?;
    }
    let _lock = StartupLock::acquire(endpoint)?;

    let mut attempts = 0;
    loop {
        attempts += 1;
        match forward(endpoint, messages) {
            Ok(()) => return Ok(Instance::Forwarded),
            // Nobody is listening: free to take the socket.
            Err(IpcError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) => {}
            // Someone is there but did not cooperate: do not start a second instance.
            Err(error) => return Err(error),
        }
        // Only now is it safe to remove a socket file left by a crashed instance.
        if let Endpoint::Path(path) = endpoint {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match bind(endpoint) {
            Ok(listener) => {
                return Ok(Instance::Primary(serve(
                    endpoint, listener, tx, on_message,
                )?));
            }
            // Lost a race (named pipes): the winner is reachable now, go round again and forward.
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempts < 5 => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// [`start_or_forward`] for a process that an update has just started: the old version still holds
/// the instance, so "another instance is running" means "it has not quit yet". Retries for up to
/// `wait` (forwarding only the harmless empty "bring your window forward" message, never `messages`)
/// and becomes the primary instance as soon as the old one is gone. If it is still there when the
/// wait ends, this returns [`Instance::Forwarded`] and the caller exits like any second launch.
pub fn start_or_wait(
    endpoint: &Endpoint,
    messages: &[String],
    tx: &Sender<String>,
    on_message: impl Fn() + Send + Sync + Clone + 'static,
    wait: Duration,
) -> Result<Instance, IpcError> {
    let deadline = Instant::now() + wait;
    loop {
        let waiting = Instant::now() < deadline;
        // While waiting, nothing but a focus request may reach the old instance.
        let sent: &[String] = if waiting { &[] } else { messages };
        match start_or_forward(endpoint, sent, tx.clone(), on_message.clone()) {
            // Still there (it answered) or quitting in the middle of our focus request (a half-read
            // frame, a refused connection): either way that is what is being waited for.
            Ok(Instance::Forwarded) | Err(_) if waiting => {
                thread::sleep(Duration::from_millis(250));
            }
            other => return other,
        }
    }
}

/// Create the listener **without** `try_overwrite`: that option deletes the socket file whenever the
/// address is in use, which would steal a live instance's socket.
fn bind(endpoint: &Endpoint) -> io::Result<interprocess::local_socket::Listener> {
    ListenerOptions::new().name(endpoint.name()?).create_sync()
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
    #[cfg(unix)]
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
        fn a_relaunched_process_waits_for_the_old_instance_and_then_takes_over() {
            let dir = short_dir("wait");
            let endpoint = endpoint(&dir);
            let (old, _rx) = primary(&endpoint);
            let leaving = thread::spawn(move || {
                thread::sleep(Duration::from_millis(600));
                drop(old); // the old version quits
            });
            let (tx, _rx2) = mpsc::channel();
            let started = Instant::now();
            let instance = start_or_wait(
                &endpoint,
                &["x.torrent".into()],
                &tx,
                || {},
                Duration::from_secs(10),
            )
            .unwrap();
            assert!(
                matches!(instance, Instance::Primary(_)),
                "it took over once the old one was gone"
            );
            assert!(
                started.elapsed() >= Duration::from_millis(500),
                "it did wait"
            );
            leaving.join().unwrap();
        }

        #[test]
        fn a_relaunched_process_gives_up_when_the_old_instance_never_quits() {
            let dir = short_dir("stuck");
            let endpoint = endpoint(&dir);
            let (_old, rx) = primary(&endpoint);
            let (tx, _rx2) = mpsc::channel();
            let instance = start_or_wait(
                &endpoint,
                &["magnet:?xt=urn:btih:abc".into()],
                &tx,
                || {},
                Duration::from_millis(700),
            )
            .unwrap();
            assert!(matches!(instance, Instance::Forwarded));
            // Only focus requests were sent while waiting; the real message goes through at the end.
            let mut received = Vec::new();
            while let Ok(message) = rx.recv_timeout(Duration::from_millis(300)) {
                received.push(message);
            }
            assert_eq!(
                received.last().map(String::as_str),
                Some("magnet:?xt=urn:btih:abc")
            );
            let before_last = received.len().saturating_sub(1);
            assert!(
                received.iter().take(before_last).all(String::is_empty),
                "{received:?}"
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
        fn a_live_socket_is_never_stolen() {
            let dir = short_dir("steal");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);
            // Binding the same address again must fail rather than delete the live socket.
            let error = bind(&endpoint).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
            // The original instance still works.
            forward(&endpoint, &["still-mine".into()]).unwrap();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "still-mine"
            );
        }

        #[test]
        fn simultaneous_launches_produce_exactly_one_primary() {
            let dir = short_dir("race");
            let endpoint = Arc::new(endpoint(&dir));
            let (tx, rx) = mpsc::channel();
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let (endpoint, tx, barrier) =
                        (Arc::clone(&endpoint), tx.clone(), Arc::clone(&barrier));
                    thread::spawn(move || {
                        barrier.wait();
                        start_or_forward(&endpoint, &[format!("launch-{i}")], tx, || {}).unwrap()
                    })
                })
                .collect();
            let instances: Vec<Instance> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            let primaries = instances
                .iter()
                .filter(|i| matches!(i, Instance::Primary(_)))
                .count();
            assert_eq!(
                primaries, 1,
                "exactly one launch may become the primary instance"
            );
            // Everyone else forwarded: seven messages reached the primary.
            let mut received = Vec::new();
            while let Ok(message) = rx.recv_timeout(Duration::from_secs(2)) {
                received.push(message);
            }
            assert_eq!(received.len(), 7, "{received:?}");
        }

        #[test]
        fn an_unresponsive_instance_is_not_replaced() {
            let dir = short_dir("hung");
            ensure_private_dir(&dir.0).unwrap();
            // A "primary" that accepts connections into its backlog but never answers.
            let hung = std::os::unix::net::UnixListener::bind(dir.0.join("ipc.sock")).unwrap();
            let (tx, _rx) = mpsc::channel();
            let error = start_or_forward(&endpoint(&dir), &["x".into()], tx, || {}).unwrap_err();
            assert!(matches!(error, IpcError::Timeout), "{error:?}");
            assert!(
                dir.0.join("ipc.sock").exists(),
                "the other instance's socket must not be deleted"
            );
            drop(hung);
        }

        #[test]
        fn the_connection_cap_drops_excess_clients_and_recovers() {
            let dir = short_dir("cap");
            let endpoint = endpoint(&dir);
            let (_server, rx) = primary(&endpoint);
            let silent: Vec<Stream> = (0..MAX_CONNECTIONS + 2)
                .map(|_| Stream::connect(endpoint.name().unwrap()).unwrap())
                .collect();
            thread::sleep(Duration::from_millis(300));
            // All slots are busy waiting on silent clients: a legitimate client is turned away…
            assert!(forward(&endpoint, &["too-early".into()]).is_err());
            // …and works again once the silent ones have timed out.
            thread::sleep(IO_DEADLINE + Duration::from_millis(500));
            forward(&endpoint, &["later".into()]).unwrap();
            drop(silent);
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "later");
            assert!(rx.try_recv().is_err());
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
