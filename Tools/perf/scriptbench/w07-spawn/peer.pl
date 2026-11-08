my $ok = 0;
for (1 .. 100) {
    $ok += 1 if system("true") == 0;
}
print "children ok=$ok\n";
