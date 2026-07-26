# Tower orchestration cleanup

Removed the stale cached `tower-burndown` skill. It treated burndown as ranking only.

Updated the remaining cached Tower instructions. They now name `tower-burndown` as the multi-card execution skill.

Repaired 14 archived Tower log fields through `tower repair apply`. Each stale label now says `burndown`.

Confirmed that no stale label remains in the repository, Tower plugin cache, or Codex skills directory.
