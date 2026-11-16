use strict;
use warnings;
use File::Find;

my ($files, $bytes) = (0, 0);
find({ no_chdir => 1, wanted => sub {
    return unless -f $_;
    $files += 1;
    $bytes += -s _;
} }, "w02");
print "files=$files bytes=$bytes\n";
