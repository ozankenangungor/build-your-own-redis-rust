use crate::resp::Value;
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// The replicas following this master, and the way to reach them.
///
/// A replica is only ever written to, never waited on, so each one is held as
/// the sending end of a queue. The connection it came in on does the writing,
/// which keeps a replica that has stopped reading from holding up the client
/// whose command is being passed along.
#[derive(Clone, Default)]
pub struct Replicas(Arc<Mutex<Vec<mpsc::UnboundedSender<Bytes>>>>);

impl Replicas {
    /// Takes on a replica, handing back the end of the queue its connection is
    /// to write out.
    pub fn add(&self) -> mpsc::UnboundedReceiver<Bytes> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.senders().push(sender);

        receiver
    }

    /// How many replicas are being kept up to date.
    pub fn count(&self) -> usize {
        self.senders().len()
    }

    /// Passes a command on to every replica, in the order they were given.
    pub fn send(&self, command: &Value) {
        let mut senders = self.senders();
        if senders.is_empty() {
            return;
        }

        let encoded = Bytes::from(command.encode());
        // A replica whose connection has ended is dropped rather than kept and
        // written to for as long as the master runs.
        senders.retain(|sender| sender.send(encoded.clone()).is_ok());
    }

    fn senders(&self) -> std::sync::MutexGuard<'_, Vec<mpsc::UnboundedSender<Bytes>>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
