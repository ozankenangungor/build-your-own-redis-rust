# Redis, written in Rust

A Redis server built from scratch, following the
["Build Your Own Redis"](https://codecrafters.io/challenges/redis) challenge.
It speaks the real Redis wire protocol, so `redis-cli` and any Redis client
library can talk to it.

## Running it

```sh
cargo run          # listens on 127.0.0.1:6379
redis-cli PING     # from another terminal
```

## What it supports

| Area | Commands |
| --- | --- |
| Connection | `PING`, `ECHO` |
| Strings | `SET` (with `EX` / `PX` expiry), `GET` |
| Lists | `RPUSH`, `LPUSH`, `LRANGE`, `LLEN`, `LPOP`, `BLPOP` |
| Streams | `XADD`, `XRANGE`, `XREAD` (with `BLOCK` and `$`) |
| Keys | `TYPE` |

Beyond the individual commands, the server:

- serves any number of clients at once, one task per connection;
- handles several commands arriving in a single packet, and commands split
  across packets;
- expires keys lazily, the way Redis does;
- blocks clients on `BLPOP` and `XREAD BLOCK`, waking the one that has waited
  longest for a list element and every waiting reader for a stream entry;
- replies with the same error messages as Redis, including `WRONGTYPE` and the
  arity and syntax errors.

## Layout

```
src/
  main.rs         binds the port and accepts connections
  connection.rs   reads commands from one client and writes the replies
  resp.rs         the Redis serialization protocol: parsing and encoding
  commands/       one module per command family, plus the dispatcher
  store/          the shared key-value state, split by data type
tests/            integration tests that drive a real server over TCP
```

## Tests

The test suite starts the compiled binary and talks to it over a socket, so it
exercises the same path a real client takes.

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
```
