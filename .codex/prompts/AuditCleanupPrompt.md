Perform an audit and analysis of the project's code structure. Create a workflow
that improves maintainability and code quality while preserving behavior. Favor
modules and subdirectories that make the codebase easier to navigate and keep
files narrowly scoped.

Hard rule: no feature breakage. Jet must be fully functional before and after the
refactor. Verify with the repo's Nix-based build and test workflow.
