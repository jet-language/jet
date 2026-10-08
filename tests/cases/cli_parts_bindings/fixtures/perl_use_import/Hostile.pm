package Hostile;
BEGIN {
    open my $file, '>', $ENV{PERL_BIND_USE_MARKER} or die $!;
    print $file "executed\n";
}
1;
