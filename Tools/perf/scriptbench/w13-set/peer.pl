use strict;
use warnings;

my %seen;
open(my $handle, "<", "w13.txt") or die "open: $!";
while (my $line = <$handle>) {
    chomp $line;
    $seen{$line} = 1;
}
print "unique=", scalar(keys %seen), "\n";
