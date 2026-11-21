# image.sh JET FACT IMAGE: one fact of a built image, read with
# `jet debug IMAGE --do COMMAND` and no external disassembler or symbol
# lister.
#   order          the names of `__jet_start` and `jet_perf_zone_close`, in
#                  the order of their addresses (`symbols`)
#   code-segments  how many load segments are read+execute (`segments`)
#   zones-loads    how many instructions load a value named by the runtime's
#                  zones word (`jet_rt_perf_zones`) into r11 (`disas`)
# A listing `jet debug` refuses exits 3 and prints nothing.
jet=$1
case $2 in
order) command=symbols ;;
code-segments) command=segments ;;
zones-loads) command=disas ;;
*) echo "image.sh: no fact $2" >&2; exit 2 ;;
esac
"$jet" debug "$3" --do $command > listing.txt || exit 3
case $2 in
order) awk '$4 == "__jet_start" || $4 == "jet_perf_zone_close" { print $4 }' listing.txt ;;
code-segments) awk '$1 == "LOAD" && $11 == "r-x" { n++ } END { print n + 0 }' listing.txt ;;
zones-loads) awk '/ r11,.*\[rip[-+]0x[0-9a-f]+\].*<jet_rt_perf_zones[>+]/ { n++ } END { print n + 0 }' listing.txt ;;
esac
