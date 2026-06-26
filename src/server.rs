use crate::config::Config;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a replication id is, in hexadecimal characters.
const ID_LENGTH: usize = 40;

/// What this server is: how it was asked to run, and where it stands among the
/// servers replicating one another.
pub struct Server {
    pub config: Config,
    pub replication: Replication,
}

/// A server's place in the history of a dataset.
pub struct Replication {
    /// The id this server's history is known by. A master makes a new one every
    /// time it starts, which is how a replica tells the history it was
    /// following from the one it finds after a restart.
    pub id: String,
    /// How many bytes of commands have been handed to replicas so far.
    pub offset: u64,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            replication: Replication {
                id: new_id(),
                offset: 0,
            },
        }
    }
}

/// Makes up a replication id.
///
/// It only has to be unlikely to be met twice, not unguessable, since it names
/// a history rather than guards it: the time and the process it was made in are
/// enough to tell two servers apart.
fn new_id() -> String {
    const HEX: &[u8] = b"0123456789abcdef";

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    let mut state = now ^ (u64::from(std::process::id()) << 32);

    (0..ID_LENGTH)
        .map(|_| {
            // Xorshift, which is a poor source of randomness and an ample one
            // for a name. A zero state would only ever beget itself.
            state = state.max(1);
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            HEX[(state & 0xf) as usize] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_an_id_of_the_length_redis_uses() {
        assert_eq!(new_id().len(), 40);
    }

    #[test]
    fn makes_an_id_of_hexadecimal_characters() {
        assert!(new_id().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn makes_a_different_id_each_time() {
        let ids: Vec<String> = (0..100).map(|_| new_id()).collect();

        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();

        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn starts_a_master_at_the_beginning_of_its_history() {
        assert_eq!(Server::new(Config::default()).replication.offset, 0);
    }
}
