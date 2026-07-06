// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A running instance of the server, killed when it goes out of scope.
pub struct Server {
    process: Child,
    /// This server's own port, so that tests never talk to one another's.
    addr: String,
    /// What the server has said about itself since it started.
    logs: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// Starts the server on a port of its own and waits until it is listening.
    ///
    /// The port is left to the operating system and read back from the server,
    /// rather than picked here: a port found free a moment ago may have been
    /// taken by the time the server reaches for it.
    pub fn start() -> Self {
        Self::start_with(&[])
    }

    /// The same, for a server that needs telling something on top of its port.
    pub fn start_with(args: &[&str]) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_codecrafters-redis"))
            .args(["--port", "0"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn server");

        let logs = process.stderr.take().expect("server stderr was piped");

        // Reaped by `Drop` on every path, including a panic in `port_from`.
        let mut server = Self {
            process,
            addr: String::new(),
            logs: Arc::new(Mutex::new(Vec::new())),
        };

        let port = port_from(logs, Arc::clone(&server.logs));
        server.addr = format!("127.0.0.1:{port}");

        server
    }

    /// What the server has said about itself so far. Waits a moment first, so
    /// that a test asking about something that has just happened elsewhere is
    /// not asking too early.
    pub fn logs(&self) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(300));

        self.logs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The port this server ended up on, which a test may need to recognise it
    /// by when it introduces itself elsewhere.
    pub fn port(&self) -> u16 {
        self.addr
            .rsplit(':')
            .next()
            .and_then(|port| port.parse().ok())
            .expect("the address holds a port")
    }

    pub fn connect(&self) -> Client {
        let stream = TcpStream::connect(&self.addr).expect("failed to connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("failed to set read timeout");
        Client(stream)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Takes a connection through the handshake a replica makes, handing back the
/// replica's end of it and the point in the master's history it starts from.
pub fn follow(server: &Server) -> (Client, u64) {
    let mut replica = server.connect();

    replica.send(&["PING"]);
    replica.expect_reply("+PONG\r\n");
    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["PSYNC", "?", "-1"]);

    let agreement = replica.read_line();
    let from = agreement
        .rsplit(' ')
        .next()
        .and_then(|offset| offset.parse().ok())
        .unwrap_or_else(|| panic!("no offset in {agreement:?}"));

    replica.read_file();

    (replica, from)
}

/// Stands in for a replica that keeps up with its master: it takes in whatever
/// it is sent and says how far it has got whenever it is asked.
///
/// A master only asks while a client is waiting on the answer, so this has to
/// answer from a thread of its own.
pub struct FakeReplica {
    /// A second hold on the connection, kept only so that dropping the replica
    /// hangs up on the master and stirs the thread out of its reading.
    hangup: TcpStream,
}

impl FakeReplica {
    pub fn follow(server: &Server) -> Self {
        let (replica, from) = follow(server);
        let hangup = replica
            .0
            .try_clone()
            .expect("failed to hold the connection");

        std::thread::spawn(move || keep_up(replica, from));

        Self { hangup }
    }
}

impl Drop for FakeReplica {
    fn drop(&mut self) {
        let _ = self.hangup.shutdown(Shutdown::Both);
    }
}

/// Takes in what the master sends, counting the bytes of it, and answers each
/// asking with how far it had got when the asking arrived.
fn keep_up(mut replica: Client, from: u64) {
    let mut taken_in = from;

    while let Some(command) = replica.try_read_reply() {
        if command.to_uppercase().contains("GETACK") {
            replica.send(&["REPLCONF", "ACK", &taken_in.to_string()]);
        }

        taken_in += command.len() as u64;
    }
}

/// Stands in for a master, so that what a replica says to one can be read.
pub struct FakeMaster(TcpListener);

impl FakeMaster {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to listen as a master");
        listener
            .set_nonblocking(true)
            .expect("failed to stop the listener from blocking");

        Self(listener)
    }

    pub fn port(&self) -> u16 {
        self.0
            .local_addr()
            .expect("a bound listener has an address")
            .port()
    }

    /// Waits for the replica to introduce itself, rather than blocking for good
    /// if it never does.
    pub fn accept(&self) -> Client {
        let deadline = Instant::now() + Duration::from_secs(10);

        while Instant::now() < deadline {
            if let Ok((stream, _)) = self.0.accept() {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("failed to set read timeout");
                return Client(stream);
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        panic!("no replica connected");
    }

    /// Asserts that nobody comes knocking, for a server that should not be
    /// following anyone.
    pub fn expect_no_one(&self) {
        std::thread::sleep(Duration::from_millis(500));

        assert!(
            self.0.accept().is_err(),
            "something connected to the master"
        );
    }
}

/// Reads the port the server announces once it is listening, then leaves a
/// thread gathering the rest of what it says. Something has to keep reading:
/// a full pipe would bring the server to a halt.
fn port_from(logs: impl Read + Send + 'static, into: Arc<Mutex<Vec<String>>>) -> u16 {
    let mut logs = BufReader::new(logs);
    let mut line = String::new();

    let port = loop {
        line.clear();
        let read = logs
            .read_line(&mut line)
            .expect("failed to read server logs");
        assert!(read > 0, "server stopped before it started listening");

        if let Some(port) = line.trim().strip_prefix("listening on port ") {
            break port.parse().expect("a port is a number");
        }
    };

    std::thread::spawn(move || {
        for line in logs.lines().map_while(Result::ok) {
            into.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    });

    port
}

/// A client connection used to send commands and assert on the replies.
pub struct Client(TcpStream);

impl Client {
    /// Sends `args` encoded as a RESP array, the way a real Redis client does.
    pub fn send(&mut self, args: &[&str]) {
        let args: Vec<&[u8]> = args.iter().map(|arg| arg.as_bytes()).collect();
        self.send_bytes(&args);
    }

    /// The same, for arguments that are not text.
    pub fn send_bytes(&mut self, args: &[&[u8]]) {
        let mut request = format!("*{}\r\n", args.len()).into_bytes();
        for arg in args {
            request.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
            request.extend_from_slice(arg);
            request.extend_from_slice(b"\r\n");
        }
        self.send_raw(&request);
    }

    /// Sends bytes without minding whether they all get through, for input the
    /// server is meant to turn away part way into.
    pub fn try_send_raw(&mut self, bytes: &[u8]) {
        let _ = self.0.write_all(bytes);
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.0.write_all(bytes).expect("failed to send command");
    }

    /// Reads as many bytes as `expected` is long and asserts they match. Reading
    /// a fixed length keeps the assertion independent of how the reply is split
    /// across TCP reads.
    pub fn expect_reply(&mut self, expected: &str) {
        let mut buf = vec![0u8; expected.len()];
        self.0.read_exact(&mut buf).expect("failed to read reply");
        assert_eq!(String::from_utf8_lossy(&buf), expected);
    }

    /// The same, for replies that are not text.
    pub fn expect_bytes(&mut self, expected: &[u8]) {
        let mut buf = vec![0u8; expected.len()];
        self.0.read_exact(&mut buf).expect("failed to read reply");
        assert_eq!(buf, expected);
    }

    /// Reads one command without checking what it is, for the steps of a
    /// conversation a test is only passing through.
    pub fn read_command(&mut self) -> Vec<String> {
        let header = self.read_line();
        let count: usize = header
            .strip_prefix('*')
            .unwrap_or_else(|| panic!("expected an array, got {header:?}"))
            .parse()
            .expect("array length");

        (0..count).map(|_| self.read_bulk_string()).collect()
    }

    /// Asserts that nothing more arrives, for a party that should be waiting to
    /// be spoken to.
    pub fn expect_silence(&mut self) {
        self.0
            .set_read_timeout(Some(Duration::from_millis(300)))
            .expect("failed to set read timeout");

        let mut buf = [0u8; 64];
        match self.0.read(&mut buf) {
            Ok(0) => {}
            Ok(read) => panic!("heard {:?}", String::from_utf8_lossy(&buf[..read])),
            Err(_) => {}
        }
    }

    /// Reads one reply of any shape, as the bytes it arrived in.
    pub fn read_reply(&mut self) -> String {
        let line = self.read_line();

        match line.as_bytes().first() {
            Some(b'+' | b'-' | b':') => format!("{line}\r\n"),
            Some(b'$') => {
                let length: i64 = line[1..].parse().expect("bulk string length");
                if length < 0 {
                    return format!("{line}\r\n");
                }

                let mut rest = vec![0u8; length as usize + 2];
                self.0.read_exact(&mut rest).expect("failed to read reply");

                format!("{line}\r\n{}", String::from_utf8_lossy(&rest))
            }
            Some(b'*') => {
                let count: i64 = line[1..].parse().expect("array length");
                if count < 0 {
                    return format!("{line}\r\n");
                }

                (0..count).fold(format!("{line}\r\n"), |reply, _| reply + &self.read_reply())
            }
            _ => panic!("not a reply: {line:?}"),
        }
    }

    /// The same as `read_reply`, but hands back nothing rather than failing
    /// when the other end has gone.
    pub fn try_read_reply(&mut self) -> Option<String> {
        let mut byte = [0u8; 1];
        self.0.set_read_timeout(None).ok()?;
        match self.0.peek(&mut byte) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }

        Some(self.read_reply())
    }

    /// Sends a command over and over until the reply is the one expected.
    ///
    /// A replica is told what changed on a connection of its own, so a client
    /// asking about it may be asking before word has reached it.
    pub fn expect_reply_eventually(&mut self, command: &[&str], expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            self.send(command);
            let reply = self.read_reply();

            if reply == expected {
                return;
            }
            assert!(Instant::now() < deadline, "still answering {reply:?}");

            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Reads a file, which is laid out like a bulk string but for the CRLF it
    /// does not end in.
    pub fn read_file(&mut self) -> Vec<u8> {
        let header = self.read_line();
        let length: usize = header
            .strip_prefix('$')
            .unwrap_or_else(|| panic!("expected a file, got {header:?}"))
            .parse()
            .expect("file length");

        let mut file = vec![0u8; length];
        self.0.read_exact(&mut file).expect("failed to read a file");

        file
    }

    /// Reads a bulk string reply and returns its contents, for the replies whose
    /// exact bytes cannot be known in advance.
    pub fn read_bulk_string(&mut self) -> String {
        let header = self.read_line();
        let length: usize = header
            .strip_prefix('$')
            .unwrap_or_else(|| panic!("expected a bulk string, got {header:?}"))
            .parse()
            .expect("bulk string length");

        let mut buf = vec![0u8; length + 2];
        self.0.read_exact(&mut buf).expect("failed to read reply");

        String::from_utf8_lossy(&buf[..length]).into_owned()
    }

    pub fn read_line(&mut self) -> String {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            self.0.read_exact(&mut byte).expect("failed to read reply");
            if byte[0] == b'\r' {
                self.0.read_exact(&mut byte).expect("failed to read reply");
                return String::from_utf8_lossy(&line).into_owned();
            }
            line.push(byte[0]);
        }
    }
}
