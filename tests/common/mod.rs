// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A running instance of the server, killed when it goes out of scope.
pub struct Server {
    process: Child,
    /// This server's own port, so that tests never talk to one another's.
    addr: String,
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
        };
        server.addr = format!("127.0.0.1:{}", port_from(logs));

        server
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

/// Reads the port the server announces once it is listening, then leaves a
/// thread to swallow the rest of its logs so that a full pipe can never bring
/// the server to a halt.
fn port_from(logs: impl Read + Send + 'static) -> u16 {
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

    std::thread::spawn(move || std::io::copy(&mut logs, &mut std::io::sink()));

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
