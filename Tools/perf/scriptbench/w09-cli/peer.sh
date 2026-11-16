set -euo pipefail
command=${1:-}
shift || true
name="" times=1 shout=0 a=0 b=0
while [[ $# -gt 0 ]]; do
  case $1 in
    --name) name=$2; shift 2 ;;
    --times) times=$2; shift 2 ;;
    --shout) shout=1; shift ;;
    --a) a=$2; shift 2 ;;
    --b) b=$2; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
case $command in
  greet)
    [[ -n $name ]] || { echo "--name is required" >&2; exit 2; }
    text="Hello, $name!"
    [[ $shout == 1 ]] && text=${text^^}
    for ((i = 0; i < times; i++)); do echo "$text"; done ;;
  add) echo $((a + b)) ;;
  *) echo "usage: main <greet|add> [options]" >&2; exit 2 ;;
esac
