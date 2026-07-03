use crate::resp::Value;
use bytes::Bytes;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{Notify, futures::Notified, mpsc};

/// The replicas following this master, and the way to reach them.
///
/// A replica is written to rather than waited on, so each one is held as the
/// sending end of a queue. The connection it came in on does the writing, which
/// keeps a replica that has stopped reading from holding up the client whose
/// command is being passed along.
#[derive(Clone, Default)]
pub struct Replicas(Arc<State>);

#[derive(Default)]
struct State {
    following: Mutex<Vec<Replica>>,
    /// Woken whenever a replica says how far it has got, so that whoever is
    /// waiting on one to catch up hears about it at once.
    stirred: Notify,
}

struct Replica {
    changes: mpsc::UnboundedSender<Bytes>,
    /// How much of the stream this replica has said it has taken in. It is the
    /// replica's connection that hears this and writes it down.
    reached: Arc<AtomicU64>,
}

impl Replicas {
    /// Takes on a replica, handing back the end of the queue its connection is
    /// to write out, and where that connection is to note how far it has got.
    pub fn add(&self) -> (mpsc::UnboundedReceiver<Bytes>, Arc<AtomicU64>) {
        let (changes, receiver) = mpsc::unbounded_channel();
        let reached = Arc::new(AtomicU64::new(0));

        self.following().push(Replica {
            changes,
            reached: Arc::clone(&reached),
        });

        (receiver, reached)
    }

    /// How many replicas are being kept up to date.
    pub fn count(&self) -> usize {
        self.following().len()
    }

    /// How many replicas have said they are at least this far along.
    pub fn caught_up_to(&self, offset: u64) -> usize {
        self.following()
            .iter()
            .filter(|replica| replica.reached.load(Ordering::Relaxed) >= offset)
            .count()
    }

    /// Notes how far a replica says it has got, and wakes whoever is waiting on
    /// that news.
    pub fn reached(&self, replica: &AtomicU64, offset: u64) {
        replica.store(offset, Ordering::Relaxed);
        self.0.stirred.notify_waiters();
    }

    /// Waits for a replica to say how far it has got.
    ///
    /// The future is made before the count is looked at, so that news arriving
    /// in between is waited on rather than missed.
    pub fn stirred(&self) -> Notified<'_> {
        self.0.stirred.notified()
    }

    /// Passes a command on to every replica, in the order they were given, and
    /// says how many bytes of the stream it took up.
    pub fn send(&self, command: &Value) -> u64 {
        let mut following = self.following();
        if following.is_empty() {
            return 0;
        }

        let encoded = Bytes::from(command.encode());
        // A replica whose connection has ended is dropped rather than kept and
        // written to for as long as the master runs.
        following.retain(|replica| replica.changes.send(encoded.clone()).is_ok());

        encoded.len() as u64
    }

    fn following(&self) -> MutexGuard<'_, Vec<Replica>> {
        self.0
            .following
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
