use strict;
use warnings;
use Getopt::Long;

my $command = shift @ARGV // "";
if ($command eq "greet") {
    my ($name, $times, $shout) = (undef, 1, 0);
    GetOptions("name=s" => \$name, "times=i" => \$times, "shout" => \$shout) or exit 2;
    die "--name is required\n" unless defined $name;
    my $text = "Hello, $name!";
    print $shout ? uc($text) : $text, "\n" for 1 .. $times;
} elsif ($command eq "add") {
    my ($a, $b);
    GetOptions("a=i" => \$a, "b=i" => \$b) or exit 2;
    print $a + $b, "\n";
} else {
    die "usage: main <greet|add> [options]\n";
}
