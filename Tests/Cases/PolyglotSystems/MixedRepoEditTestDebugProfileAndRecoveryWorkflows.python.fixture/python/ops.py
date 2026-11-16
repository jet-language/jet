def add(left: int, right: int) -> int:
    return left + right

def negate(value: bool) -> bool:
    return not value

def fail(value: int) -> int:
    raise RuntimeError('hidden Python detail')

def wrong_type(value: int) -> int:
    return 1.5
