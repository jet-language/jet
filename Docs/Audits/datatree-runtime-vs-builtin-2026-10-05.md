# DataTree: runtime library vs builtin (2026-10-05)

Scope: differences between the runtime DataTree (std_data_tree.jet, DataTree.rs) and the builtin DataTree, as recorded on 2026-10-05.

Differences recorded:

- Duplicate keys handling.
- Number lexeme representation.
- TypedText vs Text.
- Bytes variant.
- Error kind names.

The core-library documentation contradicts itself: core-library.md:2301 and core-library.md:2158 disagree. The decision on which behavior is canonical is open (owner ballot pending). Source: Idiom-Runtime3/DATATREE-DIFF.md.
