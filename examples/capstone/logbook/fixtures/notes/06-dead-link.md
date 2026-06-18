---
name: dead-link-demo
description: demonstrates a dead wikilink for lint testing
metadata:
  type: project
---
This note contains a link to a target that does not exist: [[dead-target-xyz]].

It also links to a valid note: [[jet-philosophy]].

The dead link above should be caught by the linter and reported as an unresolved reference.

#lint
