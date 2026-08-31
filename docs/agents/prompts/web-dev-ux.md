# Web Dev UX

## Goal

The goal is to identify all the quality of life features, ux/ui features, tools, infra/etc. that make web dev so smooth, fun, and user friendly. We need to first apply these to jet itself, but also to ensure jet has a comprable web dev experience to using typescript with something like a bun, react, tanstack stack. I really like the tanstack devtools and suite that is available for use. We should meet if not beat that experience and features.

Simple things like being able to define/run scripts/commands (which jet should already be able to do) so instead of some long or repetitive command, when in a package/project directory that has a run entrypoint, just being able to use jet run and it auto selects the highest priority entry point. (first any file's run function if the file is named @run.jet, then the next identified fallback is whatever file at the top level has run function). Beyond that, user should specify file, do not search subdirs and run automatically. Similar with dev and build and test. 

Then more advanced things like the tanstack icon on webdev sites that let you inspect info about the site and access devtools you created as plugins, etc. 

Remember, we are not trying to just meet the current development experience for STOCK languages for each domain, but the BEST experiences for each domain (like having the awesome tanstack stack with typescript and bun) or using c++ with unreal engine, etc. NATIVE jet needs to have all the awesome features/tools/etc that the best tools in every ECOSYSTEM provide. THAT is the true end goal.