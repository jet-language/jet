def add(x: int, y: int) -> int:
    return x + y

def decorate(value: str) -> str:
    return value

def run() -> None:
    total = add(2, 5)
    print(total * 2)
    print(decorate("jet"))
