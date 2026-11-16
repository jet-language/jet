import argparse

parser = argparse.ArgumentParser(prog="main")
sub = parser.add_subparsers(dest="command", required=True)
greet = sub.add_parser("greet", help="Greet someone")
greet.add_argument("--name", required=True)
greet.add_argument("--times", type=int, default=1)
greet.add_argument("--shout", action="store_true")
add = sub.add_parser("add", help="Add two numbers")
add.add_argument("--a", type=int, required=True)
add.add_argument("--b", type=int, required=True)
args = parser.parse_args()

if args.command == "greet":
    text = f"Hello, {args.name}!"
    for _ in range(args.times):
        print(text.upper() if args.shout else text)
else:
    print(args.a + args.b)
