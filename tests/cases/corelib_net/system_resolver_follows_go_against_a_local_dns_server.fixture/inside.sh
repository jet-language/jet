# Runs as root of a fresh user, network and mount namespace: brings up the
# loopback, gives it 192.0.2.1/24 as well (a second usable IPv4 source, for
# the RFC 6724 scope rule), mounts this fixture's resolv.conf and hosts over
# /etc's, then runs the built program.
set -eu
ip link set lo up
ip addr add 192.0.2.1/24 dev lo
mount --bind resolv.conf /etc/resolv.conf
mount --bind hosts /etc/hosts
exec ./.jet/build/main
