use strict;
use warnings;
use Time::HiRes qw(usleep);

usleep(100000) for 1 .. 30;
print "slept 30\n";
