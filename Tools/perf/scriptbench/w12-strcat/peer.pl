my $s = "";
$s .= "abc," for 1 .. 2000000;
print "len=", length($s), "\n";
