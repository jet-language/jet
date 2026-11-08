use strict;
use warnings;

my $text = join("\n", map { "alpha beta gamma delta epsilon $_" } 0 .. 19999);
my ($count, $long, $bytes) = (0, 0, 0);
for my $line (split /\n/, $text) {
    for my $word (split / /, $line) {
        $count += 1;
        $bytes += length($word);
        $long += 1 if length($word) > 4;
    }
}
print "words=$count long=$long bytes=$bytes\n";
