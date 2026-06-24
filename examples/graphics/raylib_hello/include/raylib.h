/* Minimal raylib subset, vendored for Jet's native C binder (D-CBIND3,
 * Source/CBind.rs). Only prototypes inside Jet's bindable FFI subset (scalars,
 * `bool`, `char*` -> String, `void`) appear here, with raylib's `RLAPI` macro
 * and trailing `// …` comments stripped so the prototype parser reads clean
 * `<ret> name(<params>)` lines.
 *
 * Color-as-int ABI note
 * ---------------------
 * raylib's draw calls take a by-value `Color` = `struct { unsigned char r, g, b,
 * a; }` (exactly 4 bytes). The binder maps only scalars, not by-value structs,
 * so those prototypes would be dropped. We instead declare the color parameter
 * as `int`. This is ABI-correct, not a hack: a 4-byte struct of `unsigned char`
 * is classified INTEGER and passed in one general-purpose register on both
 * x86-64 SysV and AArch64 AAPCS — bit-identical to passing a `uint32_t`. The Jet
 * side packs the color little-endian as `r | (g<<8) | (b<<16) | (a<<24)`, which
 * matches the in-memory byte order of the `Color` struct's fields. The resulting
 * call is the real raylib draw call with the correct register contents.
 *
 * These prototypes bind and link against the real libraylib.so.
 */

/* Window + frame lifecycle */
void InitWindow(int width, int height, const char *title);
void CloseWindow(void);
bool WindowShouldClose(void);
void SetTargetFPS(int fps);
void BeginDrawing(void);
void EndDrawing(void);
double GetTime(void);

/* Drawing — `Color` declared as a packed little-endian RGBA `int` (see above) */
void ClearBackground(int color);
void DrawText(const char *text, int posX, int posY, int fontSize, int color);
void DrawRectangle(int posX, int posY, int width, int height, int color);
void DrawCircle(int centerX, int centerY, float radius, int color);
