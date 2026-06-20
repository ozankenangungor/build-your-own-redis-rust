use super::text;
use crate::resp::Value;
use bytes::Bytes;

/// The sections `INFO` knows how to report on, in the order it reports them.
const SECTIONS: &[&str] = &["replication"];

/// Handles the commands that ask the server about itself rather than about
/// what it is holding. `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes]) -> Option<Value> {
    let reply = match command {
        "INFO" => match args {
            // Asking for no section in particular asks for all of them.
            [] => report(SECTIONS),
            sections => {
                // A section named in bytes that are not text is a section this
                // server does not have, which is not an error either way.
                let sections: Vec<&str> = sections.iter().filter_map(|s| text(s)).collect();
                report(&sections)
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
fn report(sections: &[&str]) -> Value {
    let mut report = String::new();

    for section in sections {
        if section.eq_ignore_ascii_case("replication") {
            report.push_str("# Replication\r\n");
            // Nothing can make this server a replica yet; the role starts
            // telling the truth once `--replicaof` can point it at a master.
            report.push_str("role:master\r\n");
        }
    }

    Value::BulkString(Bytes::from(report))
}
