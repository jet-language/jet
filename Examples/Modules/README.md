# Modules

Modules, packages, imports and the Jetpack package manager.

Run a lesson from the repository root, for example `jet run Examples/Modules/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [CoreTypeImports](CoreTypeImports.jet)
- [FactValueArguments](FactValueArguments.jet) — A typed build setting is a legal generic-module value.
- [GenericModules](GenericModules.jet) — Generic module templates and aliases.
- [ImplQualifiedHead](ImplQualifiedHead/) — In an `impl` head, a package or module alias binds to the segment after it, so `Local.labels.Named` implements `labels.Named`.
- [Imports](Imports/)
- [InlineModule](InlineModule.jet) — Inline code modules — group functions AND types under a named namespace.
- [InlineModuleImports](InlineModuleImports/) — An inline module imports a sibling module under its own alias, which shadows the file's alias of the same name.
- [Library](Library.jet)
- [MarkerGroups](MarkerGroups/) — Fixed groups keep the original rules; imported applications name their alias.
- [MemberImportTypes](MemberImportTypes/) — A member import names any public declaration of a file module, and qualified or contextual enum spellings follow the imported enum.
- [ModuleDir](ModuleDir/) — Directory module — module banner finds banner/module.jet
- [ModuleFile](ModuleFile/) — File-based module declaration: `math: module` loads `math.jet`.
- [ModuleReexport](ModuleReexport/) — A directory module re-exports a submodule item with `pub use`.
- [PackageMembers](PackageMembers/) — A package is one namespace, even when declarations live in sibling files.
- [Packages](Packages/)
- [Prototype](Prototype/) — Loose files: this folder has no package.jet, so it is an early multi-file prototype.
- [PubConstants](PubConstants/) — 3A: a public constant is read qualified (`limits.MAX_ITEMS`) or brought into scope by name with a member import (`use limits.[GREETING]`).
- [PubFile](PubFile.jet)
- [PublishablePkg](PublishablePkg/)
- [SelectiveImports](SelectiveImports.jet)
- [SoftPublic](SoftPublic/)
- [UseBlocks](UseBlocks.jet)

## Jetpack

- [EnvironmentImage](Jetpack/EnvironmentImage/) — `env.jet` beside this manifest declares the dev environment and the shell image built from it; `config/app.toml` is the one project file the image copies in.
- [Epoch5](Jetpack/Epoch5/)
