BEGIN { system("touch compile-time-marker") }
sub Safe { return $_[0]; }
1;
