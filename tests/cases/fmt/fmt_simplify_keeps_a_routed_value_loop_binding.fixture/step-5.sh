x=$(cat loop/plain.out); y=$(cat loop/run.jet); [ "${x#*fn search}" != "$x" ] && [ "${x#*fn search}" = "${y#*fn search}" ] && echo SAME
