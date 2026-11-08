import json
from dataclasses import dataclass


@dataclass
class Person:
    id: int
    name: str
    dept: str
    salary: int
    active: bool
    tags: list


with open("w06.json") as handle:
    people = [Person(**item) for item in json.load(handle)]
active = [p for p in people if p.active]
print(f"records={len(people)} active={len(active)} salary={sum(p.salary for p in active)}")
