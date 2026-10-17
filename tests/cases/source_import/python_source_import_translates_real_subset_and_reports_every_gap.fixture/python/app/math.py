import decimal

def add(x: int, y: int) -> int:
    total = x + y
    return total

def test_add() -> None:
    assert add(2, 3) == 5

def risky(x: int) -> int:
    if x > 0:
        return x
    raise RuntimeError("negative")
