/* A tiny subset of the C standard library, declared so Jet's native C-header
 * binder (D-CBIND3, Source/CBind.rs) can translate it. These are real libc
 * symbols — the Rust backend already links libc, so no extra `-l` is needed
 * beyond the `c` entry in pkg.jet's [dependencies:c].
 *
 * Only prototypes inside the bindable subset (scalars, `char*` -> String,
 * `void`) appear here; that is exactly what `jet bind` emits the cache below
 * from. This header is the human-readable source of truth for that cache.
 */

/* Length of a NUL-terminated string. `size_t` and `const char *` both land in
 * the bindable subset (Int and String respectively). */
unsigned long strlen(const char *s);

/* Absolute value of an int. Pure scalar in/out. */
int abs(int x);
