---
name: missing-field-demo
metadata:
  type: project
---
This note is intentionally missing its `description` field in the front matter.

The linter should report a MissingField error for the absent description.

#lint
