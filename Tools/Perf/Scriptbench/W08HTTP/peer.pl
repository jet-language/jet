use strict;
use warnings;
use HTTP::Tiny;

my $http = HTTP::Tiny->new;
my $total = 0;
for my $i (0 .. 199) {
    my $resp = $http->get("http://127.0.0.1:18080/item/$i");
    die "request failed" unless $resp->{success};
    $total += length $resp->{content};
}
print "requests 200\n";
print "bytes $total\n";
