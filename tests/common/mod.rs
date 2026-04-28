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

        let child = Command::new(env!("CARGO_BIN_EXE_codecrafters-redis"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn server");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(ADDR).is_ok() {
                return Self {
                    process: child,
                    _port: port,
                };
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
        self.0
            .write_all(request.as_bytes())
            .expect("failed to send command");
    }

    /// Reads the next reply and asserts it matches `expected`.
    pub fn expect_reply(&mut self, expected: &str) {
        let mut buf = [0u8; 512];
        let n = self.0.read(&mut buf).expect("failed to read reply");
        assert_eq!(String::from_utf8_lossy(&buf[..n]), expected);
    }
}
