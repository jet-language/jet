import sys
sys.path.insert(0, "target/bindings")
from projection_python import (
    PROJECTION_PYTHON_CLOSED,
    PROJECTION_PYTHON_EXPIRED_VIEW,
    PROJECTION_PYTHON_OK,
    ResourceDocument,
    load,
    project_component,
)
library = load("target/libprojection_python.so")
document = ResourceDocument(library)
assert document.open(bytes((10, 20, 30))) == PROJECTION_PYTHON_OK
status, stale = document.bytes()
assert status == PROJECTION_PYTHON_OK
assert document.at(stale, 1) == (20, PROJECTION_PYTHON_OK)
assert document.replace(bytes((40, 50, 60))) == PROJECTION_PYTHON_OK
assert document.at(stale, 1) == (0, PROJECTION_PYTHON_EXPIRED_VIEW)
status, fresh = document.bytes()
assert status == PROJECTION_PYTHON_OK
assert document.at(fresh, 1) == (50, PROJECTION_PYTHON_OK)
assert document.close() == PROJECTION_PYTHON_OK
assert document.close() == PROJECTION_PYTHON_CLOSED
assert library.label("hello") == "hello"
assert project_component(library, {"count": 7, "label": "ok"}) == {"count": 7, "label": "ok"}
assert library.add(41) == 42
