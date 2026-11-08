use strict;
use warnings;

my ($count, $total) = (0, 0);
open my $fh, "<", "data/log100m.txt" or die "open: $!";
while (my $line = <$fh>) {
    if ($line =~ /status=5\d\d ms=(\d+)/) {
        $count++;
        $total += $1;
    }
}
print "matches $count\n";
print "ms_sum $total\n";
