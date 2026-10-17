use strict;
use warnings;

my $counter = 0;

sub Transform {
    my ($input) = @_;
    $counter += 1;
    return {
        count => $counter,
        nested => $input->{nested},
        list => $input->{list},
        scalar => $input->{scalar},
        nothing => undef,
    };
}

sub Fail { die "raw secret failure detail"; }
sub Sleep { sleep 30; return $_[0]; }

1;
