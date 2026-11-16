#!/bin/sh
# A stand-in for integrate10.sh: the one stand-in body is Integrate10.jet beside it.
exec "${JET:-jet}" -A "$(dirname "$0")/Integrate10.jet" "$@"
