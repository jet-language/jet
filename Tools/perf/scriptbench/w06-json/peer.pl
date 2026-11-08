use strict;
use warnings;
use JSON::PP;

open(my $handle, "<", "w06.json") or die "open: $!";
local $/;
my $people = JSON::PP->new->decode(<$handle>);
my ($active, $salary) = (0, 0);
for my $person (@$people) {
    next unless $person->{active};
    $active += 1;
    $salary += $person->{salary};
}
printf "records=%d active=%d salary=%d\n", scalar(@$people), $active, $salary;
