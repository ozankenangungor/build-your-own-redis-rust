// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:6379";

/// The server always binds the same port, so only one may run at a time.
static PORT: Mutex<()> = Mutex::new(());

/// A running instance of the server, killed when it goes out of scope.
pub struct Server {
    process: Child,
    /// Held for as long as the server runs, to keep the port exclusive.
    _port: MutexGuard<'static, ()>,
}

impl Server {
    /// Starts the server and waits until it accepts connections.
    pub fn start() -> Self {
        let port = PORT.lock().unwrap_or_else(|e| e.into_inner());

        let process = Command::new(env!("CARGO_BIN_EXE_codecrafters-redis"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn server");

        // Reaped by `Drop` on every path, including the panic below.
        let server = Self {
            process,
            _port: port,
        };

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(ADDR).is_ok() {
                return server;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        panic!("server did not start listening on {ADDR}");
    }

    pub fn connect(&self) -> Client {
        let stream = TcpStream::connect(ADDR).expect("failed to connect to server");
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

/// A client connection used to send commands and assert on the replies.
pub struct Client(TcpStream);

impl Client {
    /// Sends `args` encoded as a RESP array, the way a real Redis client does.
    pub fn send(&mut self, args: &[&str]) {
        let mut request = format!("*{}\r\n", args.len());
        for arg in args {
            request.push_str(&format!("${}\r\n{arg}\r\n", arg.len()));
        }
        self.send_raw(request.as_bytes());
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

    fn read_line(&mut self) -> String {
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
