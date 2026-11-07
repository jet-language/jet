#!/bin/sh
# Names the directory the golden program runs from: the staged source root
# keeps the repository layout, so the entry is found by its repository path.
if [ -f Examples/topic/hello.jet ]; then printf 'source root\n'; else printf 'project\n'; fi
