#!/bin/sh
# Three TLS connections on port $1, one after another. Each gets a response
# head and the first three of ten body bytes, then nothing more: the pipe
# that feeds the server stays open, without data, until all three are done.
echo serving
writers=""
n=0
while [ "$n" -lt 3 ]; do
    n=$((n + 1))
    rm -f "stall$n.fifo"
    mkfifo "stall$n.fifo"
    { printf 'HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc'; exec sleep 600; } > "stall$n.fifo" &
    writers="$writers $!"
    openssl s_server -quiet -naccept 1 -accept "$1" -cert server.cert.pem -key server.key.pem < "stall$n.fifo"
done
kill $writers 2>/dev/null
exec sleep 600
