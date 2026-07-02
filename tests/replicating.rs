//! What a replica makes of the commands its master passes on.

mod common;

use common::{Client, FakeMaster, Server};

/// An empty dataset, as a master hands one over: a length, then the bytes, and
/// no CRLF after them.
const DATASET: &[u8] = b"$18\r\nREDIS0011\xff\0\0\0\0\0\0\0\0";

/// Plays the master through the handshake, ending with the replica ready to be
/// told what has changed.
fn hand_over(master: &FakeMaster) -> Client {
    let mut conversation = master.accept();

    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");
    conversation.send_raw(b"+PONG\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");
    conversation.read_command();
    conversation.send_raw(b"+FULLRESYNC 8371b4fb1155b71f4a04d3e1bc3e18c4d990aeeb 0\r\n");
    conversation.send_raw(DATASET);

    conversation
}

/// Starts a replica following `master`, handing back both ends.
fn following(master: &FakeMaster) -> (Server, Client) {
    let replica = Server::start_with(&["--replicaof", &format!("127.0.0.1 {}", master.port())]);
    let conversation = hand_over(master);

    (replica, conversation)
}

#[test]
fn takes_on_what_the_master_was_told_to_change() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    conversation.send(&["SET", "foo", "1"]);

    let mut client = replica.connect();
    client.expect_reply_eventually(&["GET", "foo"], "$1\r\n1\r\n");
}

#[test]
fn takes_them_on_in_the_order_they_were_made() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    conversation.send(&["SET", "counter", "1"]);
    conversation.send(&["INCR", "counter"]);
    conversation.send(&["INCR", "counter"]);

    let mut client = replica.connect();
    client.expect_reply_eventually(&["GET", "counter"], "$1\r\n3\r\n");
}

#[test]
fn answers_the_master_nothing() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    conversation.send(&["SET", "foo", "1"]);
    conversation.send(&["INCR", "foo"]);

    // The master is not waiting on a reply, and one sent would be read as
    // something else entirely.
    conversation.expect_silence();
}

#[test]
fn takes_on_commands_that_arrive_together() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    // Nothing says one command to a packet.
    conversation.send_raw(
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n*3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n",
    );

    let mut client = replica.connect();
    client.expect_reply_eventually(&["GET", "foo"], "$1\r\n1\r\n");
    client.expect_reply_eventually(&["GET", "bar"], "$1\r\n2\r\n");
}

#[test]
fn takes_on_a_command_that_arrives_in_pieces() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    for piece in [
        &b"*3\r\n$3\r\nSE"[..],
        &b"T\r\n$3\r\nfoo\r"[..],
        &b"\n$5\r\nvalue\r\n"[..],
    ] {
        conversation.send_raw(piece);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let mut client = replica.connect();
    client.expect_reply_eventually(&["GET", "foo"], "$5\r\nvalue\r\n");
}

#[test]
fn takes_on_a_dataset_that_arrives_with_the_first_command() {
    let master = FakeMaster::start();
    let replica = Server::start_with(&["--replicaof", &format!("127.0.0.1 {}", master.port())]);

    let mut conversation = master.accept();
    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");
    conversation.send_raw(b"+PONG\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");
    conversation.read_command();

    // The file ends where its bytes end rather than at a CRLF, so a command
    // sent hard on its heels must not be swallowed with it.
    let mut all = Vec::new();
    all.extend_from_slice(b"+FULLRESYNC 8371b4fb1155b71f4a04d3e1bc3e18c4d990aeeb 0\r\n");
    all.extend_from_slice(DATASET);
    all.extend_from_slice(b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n");
    conversation.send_raw(&all);

    let mut client = replica.connect();
    client.send(&["GET", "foo"]);
    client.expect_reply("$1\r\n1\r\n");
}

#[test]
fn serves_its_own_clients_while_following() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    conversation.send(&["SET", "from-master", "1"]);

    let mut client = replica.connect();
    client.send(&["SET", "from-client", "2"]);
    client.expect_reply("+OK\r\n");
    client.expect_reply_eventually(&["GET", "from-master"], "$1\r\n1\r\n");
    client.send(&["GET", "from-client"]);
    client.expect_reply("$1\r\n2\r\n");
}

#[test]
fn says_so_when_the_master_goes_away() {
    let master = FakeMaster::start();
    let (replica, conversation) = following(&master);

    drop(conversation);

    assert!(
        replica
            .logs()
            .iter()
            .any(|line| line.contains("the master hung up")),
        "{:?}",
        replica.logs(),
    );
}

#[test]
fn says_how_far_it_has_got_when_asked() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    // Nothing has come before, so it has got nowhere yet.
    conversation.send(&["REPLCONF", "GETACK", "*"]);

    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$1\r\n0\r\n");
}

#[test]
fn counts_the_asking_itself_towards_how_far_it_has_got() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    // A `REPLCONF GETACK *` is 37 bytes on the wire, and counts like anything
    // else the master sends, only not until it has been answered.
    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$1\r\n0\r\n");

    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n37\r\n");

    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n74\r\n");
}

#[test]
fn counts_what_it_was_told_that_changed_nothing() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    // A master sends these to show it is still there. They change nothing and
    // are counted all the same.
    conversation.send(&["PING"]);
    conversation.send(&["REPLCONF", "GETACK", "*"]);

    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n14\r\n");
}

#[test]
fn counts_everything_the_master_sent_in_the_order_it_came() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$1\r\n0\r\n");

    conversation.send(&["PING"]);
    conversation.send(&["REPLCONF", "GETACK", "*"]);
    // 37 for the asking already answered, and 14 for the PING.
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n51\r\n");

    conversation.send(&["SET", "foo", "1"]);
    conversation.send(&["SET", "bar", "2"]);
    conversation.send(&["REPLCONF", "GETACK", "*"]);
    // 51, another 37 for the second asking, and 29 for each SET.
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$3\r\n146\r\n");
}

#[test]
fn counts_commands_that_arrived_together() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    // How the bytes were split across packets is nothing to do with how many
    // of them there were.
    conversation.send_raw(
        b"*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n*3\r\n$8\r\nREPLCONF\r\n$6\r\nGETACK\r\n$1\r\n*\r\n",
    );

    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n28\r\n");
}

#[test]
fn says_how_far_it_has_got_however_the_asking_is_spelled() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    // Each asking counts towards the next answer, so what matters here is that
    // every spelling is answered at all.
    for (asking, offset) in [
        (vec!["REPLCONF", "GETACK", "*"], "$1\r\n0"),
        (vec!["replconf", "getack", "*"], "$2\r\n37"),
        (vec!["ReplConf", "GetAck", "*"], "$2\r\n74"),
    ] {
        conversation.send(&asking);
        conversation.expect_reply(&format!(
            "*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n{offset}\r\n"
        ));
    }
}

#[test]
fn answers_nothing_but_the_asking() {
    let master = FakeMaster::start();
    let (_replica, mut conversation) = following(&master);

    conversation.send(&["SET", "foo", "1"]);
    conversation.send(&["PING"]);
    conversation.send(&["REPLCONF", "GETACK", "*"]);

    // Only the one reply, with nothing of the two commands before it.
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n43\r\n");
    conversation.expect_silence();
}

#[test]
fn goes_on_taking_commands_in_after_being_asked() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    conversation.send(&["SET", "before", "1"]);
    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$2\r\n32\r\n");
    conversation.send(&["SET", "after", "2"]);

    let mut client = replica.connect();
    client.expect_reply_eventually(&["GET", "before"], "$1\r\n1\r\n");
    client.expect_reply_eventually(&["GET", "after"], "$1\r\n2\r\n");
}

#[test]
fn keeps_the_asking_out_of_its_own_store() {
    let master = FakeMaster::start();
    let (replica, mut conversation) = following(&master);

    conversation.send(&["REPLCONF", "GETACK", "*"]);
    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$1\r\n0\r\n");

    // Being asked is not something to carry out, so nothing of it is left.
    let mut client = replica.connect();
    client.send(&["TYPE", "REPLCONF"]);
    client.expect_reply("+none\r\n");
}
