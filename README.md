# Redis, written in Rust

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)

A Redis server built from scratch on top of Tokio, following the
["Build Your Own Redis"](https://codecrafters.io/challenges/redis) challenge.
No Redis library of any kind: the wire protocol, RDB parsing, AOF engine,
replication handshake, and geospatial indexing are all implemented from scratch.

<p align=center>
  <img src="docs/demo.gif" alt="Starting the server, then asking it for strings, sorted sets and distances with redis-cli, and finding it all still there after a restart" width="100%">
</p>

## Quick Start

```sh
# Start the server on default port (6379)
cargo run

# Or run with replication / persistence flags
cargo run -- --port 6380 --replicaof "localhost 6379"
cargo run -- --appendonly yes --appendfsync everysec

# Test with redis-cli
redis-cli PING
```

## Implemented Commands

- **Strings & Connection:** `PING`, `ECHO`, `SET` (with `EX`/`PX`), `GET`, `INCR`
- **Lists:** `RPUSH`, `LPUSH`, `LRANGE`, `LLEN`, `LPOP`, `BLPOP` (async blocking with timeout)
- **Sorted Sets & Geo:** `ZADD`, `ZRANK`, `ZRANGE`, `ZCARD`, `ZSCORE`, `ZREM`, `GEOADD`, `GEOPOS`, `GEODIST`, `GEOSEARCH`
- **Streams:** `XADD`, `XRANGE`, `XREAD` (with `BLOCK` and `$`)
- **Transactions & Pub/Sub:** `MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH`, `SUBSCRIBE`, `UNSUBSCRIBE`, `PUBLISH`
- **Replication & Persistence:** `REPLCONF`, `PSYNC`, `WAIT`, `INFO`, `KEYS`, `TYPE`, `CONFIG GET`
- **ACL:** `AUTH`, `ACL WHOAMI`, `ACL GETUSER`, `ACL SETUSER`

## Architecture & Design Notes

- **Binary-Safe Protocol:** Keys, values, channel names, and payloads are raw byte buffers (`Bytes`), supporting arbitrary binary data without UTF-8 constraints.
- **Async Concurrency:** Built on Tokio with one lightweight task per connection. Command pipelining and packet framing are fully handled.
- **Replication & Side-Effect Propagation:** The master propagates state changes rather than raw commands where they differ (e.g., a `BLPOP` is propagated as an `LPOP` so replicas never block).
- **Optimistic Concurrency Control:** `WATCH` tracks key versions per connection, aborting transactions on conflict without holding global locks during execution.
- **Geospatial Indexing:** Uses 52-bit interleaved Morton codes (geohashes) stored as sorted set scores, calculating spherical distances via the Haversine formula.
- **Multi-Part AOF & Replay:** Replays existing append-only logs on startup before arming the live writer to guarantee zero duplicate records across restarts.

## Source Layout

```
src/
  main.rs          binds the port, replays the log, accepts connections
  config.rs        server configuration & CLI parsing
  server.rs        core server state, replication metadata, users
  connection.rs    client connection handler & command loop
  resp.rs          RESP protocol serialization & deserialization
  rdb.rs           RDB binary snapshot parser
  aof.rs           multi-part AOF log writer & manifest manager
  replica.rs       master-replica synchronization handshake
  replicas.rs      connected replica tracking & write propagation
  channels.rs      pub/sub subscription registry
  users.rs         ACL & SHA-256 hashed authentication
  glob.rs          glob pattern matcher for KEYS
  commands/        command dispatchers grouped by subsystem
  store/           thread-safe in-memory storage engines
tests/             integration test suite running over real TCP sockets
```
