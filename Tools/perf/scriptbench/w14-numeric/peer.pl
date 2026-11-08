use strict;
use warnings;

my $total = 0;
$total += $_ * $_ % 7919 for 1 .. 30_000_000;
print "$total\n";
