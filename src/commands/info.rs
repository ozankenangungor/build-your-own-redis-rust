use super::text;
use crate::resp::Value;
use crate::server::Server;
use bytes::Bytes;

/// The sections `INFO` knows how to report on, in the order it reports them.
const SECTIONS: &[&str] = &["replication"];

/// Handles the commands that ask the server about itself rather than about
/// what it is holding. `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], server: &Server) -> Option<Value> {
    let reply = match command {
        "INFO" => match args {
            // Asking for no section in particular asks for all of them.
            [] => report(SECTIONS, server),
            sections => {
                // A section named in bytes that are not text is a section this
                // server does not have, which is not an error either way.
                let sections: Vec<&str> = sections.iter().filter_map(|s| text(s)).collect();
                report(&sections, server)
            }
        },
        _ => return None,
    };

    Some(reply)
}

/// Gathers the sections asked for, in the way Redis lays them out: a heading,
/// then a line of `key:value` for each thing it has to say.
///
/// A section the server does not have contributes nothing, so that asking for
/// one is answered rather than refused.
fn report(sections: &[&str], server: &Server) -> Value {
    let mut report = String::new();

    for section in sections {
        if section.eq_ignore_ascii_case("replication") {
            report.push_str("# Replication\r\n");

            // A server told to follow another is a replica, which Redis still
            // calls a slave in everything it reports.
            let role = match server.config.replicaof {
                Some(_) => "slave",
                None => "master",
            };

            let replication = &server.replication;
            report.push_str(&format!("role:{role}\r\n"));
            report.push_str(&format!("master_replid:{}\r\n", replication.id));
            report.push_str(&format!("master_repl_offset:{}\r\n", replication.offset()));
        }
    }

    Value::BulkString(Bytes::from(report))
}
