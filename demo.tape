# The demo in the README, written down so it can be made again:
#
#   vhs demo.tape
#
# Needs a Redis client on the path to do the asking. Any will do; the point of
# the demo is that this server is the one answering.

Output docs/demo.gif

Require cargo
Require redis-cli

Set Shell bash
Set FontSize 18
Set Width 1080
Set Height 360
Set Padding 22
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms

# Out of shot: build, and take the binary somewhere of its own so that nothing
# it writes lands in the working tree.
Hide
Type "export LC_ALL=C" Enter
Type "pkill -f codecrafters-redis; cargo build --release >/dev/null 2>&1" Enter
Type "D=$(mktemp -d) && cp target/release/codecrafters-redis $D/ && cd $D" Enter
Type "PS1='$ ' && clear" Enter
Show

Type "# A Redis server, written from scratch in Rust." Enter
Sleep 1.5s
Type "./codecrafters-redis --appendonly yes > server.log 2>&1 &" Enter
Sleep 2s
Type "clear" Enter
Sleep 500ms

Type "redis-cli PING" Enter
Sleep 1.2s
Type "redis-cli SET greeting 'hello from rust'" Enter
Sleep 1s
Type "redis-cli GET greeting" Enter
Sleep 2s

Type "clear" Enter
Type "# Sorted sets, held in the order their scores put them." Enter
Sleep 1.2s
Type "redis-cli ZADD racers 6.1 Ford 8.2 Royce 4.9 Sam" Enter
Sleep 1s
Type "redis-cli ZRANGE racers 0 -1" Enter
Sleep 2.2s

Type "clear" Enter
Type "# Places, kept under the geohash Redis keeps them under." Enter
Sleep 1.2s
Type "redis-cli GEOADD cities 2.2944 48.8584 Paris 13.4105 52.5244 Berlin" Enter
Sleep 1.2s
Type "redis-cli GEODIST cities Paris Berlin km" Enter
Sleep 1.8s
Type "redis-cli GEOSEARCH cities FROMLONLAT 5 50 BYRADIUS 600 km" Enter
Sleep 2.5s

Type "clear" Enter
Type "# Every change was written down as it happened." Enter
Sleep 1.2s
Type "head -c 96 appendonlydir/appendonly.aof.1.incr.aof | tr '\r\n' ' '; echo" Enter
Sleep 3s

Type "clear" Enter
Type "# So a restart picks up exactly where it left off." Enter
Sleep 1.2s
# Killed and started as two commands rather than one chain: a chain sent to the
# background is a subshell, whose job table knows nothing of %1.
Type "kill %1" Enter
Sleep 1.5s
Type "./codecrafters-redis --appendonly yes > server.log 2>&1 &" Enter
Sleep 2.5s
Type "clear" Enter
Type "grep replayed server.log" Enter
Sleep 2s
Type "redis-cli GET greeting" Enter
Sleep 1s
Type "redis-cli ZRANGE racers 0 -1" Enter
Sleep 1s
Type "redis-cli GEODIST cities Paris Berlin km" Enter
Sleep 3.5s

# The restart above leaves the new server under a job number of its own, so
# take the name rather than the number and be sure nothing is left holding the
# port for the next run.
Hide
Type "pkill -f codecrafters-redis" Enter
Sleep 500ms
Show
