use strict;
use warnings;

my %counts;
my $total = 0;
open my $fh, "<", "data/words.txt" or die "open: $!";
while (my $line = <$fh>) {
    for my $word (lc($line) =~ /[a-z]+/g) {
        $counts{$word}++;
        $total++;
    }
}
my @top = sort { $counts{$b} <=> $counts{$a} || $a cmp $b } keys %counts;
print "total $total\n";
print "distinct ", scalar(keys %counts), "\n";
print "$counts{$_} $_\n" for @top[0 .. 9];
