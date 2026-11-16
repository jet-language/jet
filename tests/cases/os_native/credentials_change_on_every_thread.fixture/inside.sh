# Runs as root of a fresh user and mount namespace that also maps uids and
# gids 1..65536: mounts this fixture's group file over /etc/group and a fresh
# directory over /var/run (so /var/run/nscd/socket is the stub's), starts the
# one-request nscd stub, then runs the program. Everything the script runs
# after the /var/run mount is a shell builtin or a built program.
set -eu
mkdir -p run/nscd
mount --bind group /etc/group
mount --bind "$PWD/run" /var/run
./nscd/.jet/build/main &
./.jet/build/main
wait
