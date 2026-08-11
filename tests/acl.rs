mod common;

use common::Server;

#[test]
fn says_who_the_client_is() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.expect_reply("$7\r\ndefault\r\n");
}

#[test]
fn says_the_same_to_every_client() {
    let server = Server::start();

    // Every connection starts out as the same user, so the answer does not
    // depend on who is asking.
    for _ in 0..3 {
        let mut client = server.connect();

        client.send(&["ACL", "WHOAMI"]);
        client.expect_reply("$7\r\ndefault\r\n");
    }
}

#[test]
fn accepts_any_casing_of_the_command_and_the_word_after_it() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [
        ["ACL", "WHOAMI"],
        ["acl", "whoami"],
        ["Acl", "WhoAmI"],
        ["ACL", "whoami"],
    ] {
        client.send(&command);
        client.expect_reply("$7\r\ndefault\r\n");
    }
}

#[test]
fn says_what_it_has_to_say_of_the_user_every_client_is() {
    let server = Server::start();
    let mut client = server.connect();

    // Two properties, each a name and its value: the flag that says the user
    // wants no password, and the passwords it has, of which there are none.
    client.send(&["ACL", "GETUSER", "default"]);
    client.expect_reply("*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n");
}

#[test]
fn says_nothing_of_a_user_it_has_never_heard_of() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "GETUSER", "alice"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn refuses_a_getuser_that_names_no_user() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "GETUSER"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Unknown ACL subcommand"), "{said:?}");
}

#[test]
fn refuses_an_acl_that_says_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL"]);
    client.expect_reply("-ERR wrong number of arguments for 'acl' command\r\n");
}

#[test]
fn refuses_a_word_it_has_no_answer_for() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "NONSENSE"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Unknown ACL subcommand"), "{said:?}");
}

#[test]
fn keeps_serving_the_connection_after_an_acl() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
