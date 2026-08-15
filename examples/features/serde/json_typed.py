"""Python differential fixture for exact typed JSON numbers."""

import json
from decimal import Decimal


payload = (
    '{"amount":12.340,"exponent":1E-5,"whole":100,'
    '"tenth":0.1,"tenth_with_zero":0.10,"scientific_tenth":1e-1,'
    '"large":12345678901234567890123456789012345678901234567890,'
    '"large_exp":1e30}'
)
value = json.loads(payload, parse_float=Decimal, parse_int=int)

assert value["amount"] == Decimal("12.340")
assert value["exponent"] == Decimal("0.00001")
assert value["whole"] == 100
assert value["tenth"] == Decimal("0.1")
assert value["tenth_with_zero"] == Decimal("0.10")
assert value["scientific_tenth"] == Decimal("0.1")
assert value["large"] == 12345678901234567890123456789012345678901234567890
assert value["large_exp"] == 10**30

accepted_nonfinite = sum(
    json.loads(token, parse_float=Decimal, parse_int=int) is not None
    for token in ("NaN", "Infinity")
)

print(value["amount"])
print(value["exponent"])
print(value["large"])
print(f"accepted-nonfinite:{accepted_nonfinite}")
