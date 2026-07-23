# Simple — before / after

## User reply

**Before**
> I've gone ahead and analyzed the constructor situation — essentially, `.{}` is great for POD-like records whereas `.new()` tends to be preferred when there's nontrivial initialization; happy to dig deeper if useful!

**After**
> The analysis is complete.
>
> Use `Type.{ … }` for plain data records.
> Use `Type.new(…)` when construction has state or special logic.
>
> Say if you want more detail on one case.

## Spec paragraph (new prose only)

**Before**
> Construction spellings are intentionally bifurcated so that beginners get an obvious literal form while experts retain explicit factory control over allocation and invariants without overloading a single syntactic mechanism.

**After**
> Jet uses two construction forms.
>
> Beginners use the struct literal `Type.{ … }` for plain data.
> Experts use `Type.new(…)` when construction allocates memory or applies special rules.

## Procedure

**Before**
> After ensuring your environment is correctly configured, you should proceed to execute the verification suite in order to validate that everything is functioning as expected.

**After**
> To verify the change:
>
> 1. Make sure you use `scripts/agent/jet-env`.
> 2. Run the targeted tests.
> 3. Read the test output.
> 4. Stop if a test fails.

## Orwell pass

**Before**
> At the end of the day, utilization of the new API will facilitate a more robust solution going forward.

**After**
> Use the new API. It is the simpler fix.

## Frozen copy (do not “simplify” unless asked)

Registered diagnostic text and ratified decision paragraphs stay as shipped.
Write new explanation beside them if needed.
