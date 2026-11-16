use strict;
use warnings;

my ($count, $qty, $value) = (0, 0, 0);
open(my $handle, "<", "w05.csv") or die "open: $!";
<$handle>;
while (my $line = <$handle>) {
    chomp $line;
    my @fields = split /,/, $line;
    $count += 1;
    $qty += $fields[3];
    $value += $fields[3] * $fields[4];
}
print "rows=$count qty=$qty value=$value\n";
