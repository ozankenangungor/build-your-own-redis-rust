# The demo in the README, written down so it can be made again.
#
#   vhs demo.tape
#
# Needs a Redis client on the path to do the asking. Any will do; the point of
# the demo is that this server is the one answering.

Output docs/demo.gif

Require cargo
Require redis-cli

Set Shell bash
Set FontSize 19
Set Width 1100
Set Height 700
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms
Set PlaybackSpeed 1.0

# Out of shot: a release build, and somewhere of its own to keep the data.
Hide
Type "cd $(mktemp -d) && cargo build --release --manifest-path $OLDPWD/Cargo.toml 2>/dev/null; PS1='$ '" Enter
Type "clear" Enter
Show

Type "# A Redis server, written from scratch in Rust." Enter
Sleep 1.2s
Type "target/release/codecrafters-redis --appendonly yes &" Enter
Sleep 2s

Type "clear" Enter
Sleep 300ms

Type "redis-cli PING" Enter
Sleep 1s
Type "redis-cli SET greeting 'hello from rust'" Enter
Sleep 800ms
Type "redis-cli GET greeting" Enter
Sleep 1.5s

Type "clear" Enter
Type "# Sorted sets, ordered by score." Enter
Sleep 800ms
Type "redis-cli ZADD racers 6.1 Ford 8.2 Royce 4.9 Sam" Enter
Sleep 800ms
Type "redis-cli ZRANGE racers 0 -1" Enter
Sleep 1.5s

Type "clear" Enter
Type "# Places, kept under the geohash Redis uses." Enter
Sleep 800ms
Type "redis-cli GEOADD cities 2.2944 48.8584 Paris 13.4105 52.5244 Berlin" Enter
Sleep 800ms
Type "redis-cli GEODIST cities Paris Berlin km" Enter
Sleep 1.2s
Type "redis-cli GEOSEARCH cities FROMLONLAT 5 50 BYRADIUS 600 km" Enter
Sleep 2s

Type "clear" Enter
Type "# Every change was written down as it happened." Enter
Sleep 1s
Type "cat appendonlydir/appendonly.aof.1.incr.aof | head -c 120" Enter
Sleep 2.5s

Type "clear" Enter
Type "# So it is still all there after a restart." Enter
Sleep 1s
Type "kill %1 && sleep 1 && target/release/codecrafters-redis --appendonly yes &" Enter
Sleep 3s
Type "clear" Enter
Type "redis-cli GET greeting" Enter
Sleep 800ms
Type "redis-cli ZRANGE racers 0 -1" Enter
Sleep 800ms
Type "redis-cli GEODIST cities Paris Berlin km" Enter
Sleep 3s

Hide
Type "kill %1" Enter
Show
