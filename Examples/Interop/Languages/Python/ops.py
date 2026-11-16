def add(left: int, right: int) -> int:
    return left + right


def fail(value: int) -> int:
    raise RuntimeError("foreign detail stays inside the sidecar")
