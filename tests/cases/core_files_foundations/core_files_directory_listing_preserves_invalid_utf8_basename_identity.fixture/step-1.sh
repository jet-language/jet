mkdir -p names && : > names/$(printf 'a\377') && : > names/$(printf 'a\200')
