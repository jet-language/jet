---
title: Owner scratch
---

"Error messages are the user interface of your compiler." Jonathan Blow https://youtu.be/e6crOMC9WCE?si=AFgqGl_HUBQ_0cRO

Want to find the video that talks about framing functions as honest/dishonest vs deterministic/nondeterministic. Want to incorporate that idea/philosophy into jet.

Consider allowing or requiring wrapping of a ctor around a factory function to make it clear/explicit when a constructor factory is used and what the output type is so users dont have to inspect the function signature. Meaning,instead of: 
```jet
myVar := Task{priority: 2, status: closed}
myConst :: create_task(2, closed)
```

```jet
myVar := Task{priority: 2, status: closed}
myConst :: Task{create_task(2, closed)}
```
I want a jet-native tool that functions like convexdb as an excellent, world class sync engine https://youtu.be/pRf8_40EDtM?si=f809dRgc4BAv-gfj

BMP Errors:
 Errors the file still has: the one jet check I ran, plus the probes, turned up these, and I haven't fixed them:
 1. BLACK :: Rgb{...}: a top-level constant can't be a struct (E0109). It needs to be a function or inlined.
 2. Int.from_radix(...) ?? fatal(...): from_radix can't fail as far as the checker is concerned, so the ?? has to go.
 3. fatal calls process.exit, which the checker treats as fallible (E0403), and it depends on the broken core.process import.
    Printing with the built-in eprint and then panicking or returning an error would avoid that import.
 4. term.stdout().write_bytes(...) doesn't exist (E0102), and core.term doesn't type-check in this tree anyway. simple needs
    another way to write bytes to stdout.
 5. It also needs an inline package { authority: { holds: { allow: [FS, IO, ...] } } } block, or writing files fails with
    E1803.
 6. Printing to stdout wasn't really available. Jet's print writes text strings, not raw bytes. The byte-writing method I tried
   (term.stdout().write_bytes) doesn't exist, and core.term doesn't type-check in this tree. Two of the three scripts also
   write to a file, so the bytes have to be built as a value somewhere.

Every jet entry point should probably be fallible, isn't every program in every language ever written? The fn run just has implicit default return error type that requires implementation of error trait?

Have allow by default with a prompt that just asks to confirm the used permissions if none are specified in the package/script/file/CLI command


-> FastAPI Video
> I like the potential idea of nested namespaces for markers like python decorators, where to make a rest api app, you can use @app.get(...), @app.post(...), etc. which is imported, then you can create the api with something like FastAPI(app, ...). Seems like overall very nice ux. 

Building A Programming Language Playlist -> Mine entire playlist
https://youtube.com/playlist?list=PLET80Nvdg3mg&si=we8LZwmFSPfQnOkA
> What can we learn from this playlist & apply to jet? What strengths, weaknesses, opportunities, and threats do we have. What can we adopt from these videos at a strategic, operational, and tactical level? What features,functions/methods, ideas, structures, components, libraries, keywords, facets, etc. can we learn from this playlist & the experience of building a language from scratch?

~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
Have Astra research videos/articles/blog posts/forum posts/etc. to find perspectives about why learning to code is hard for people. Use this to funnel into a discussion about how to address these issues from the language level. How can these issues be prevented or minimized by jet? What language level support can we provide to make learning easier (not by literally teaching, but making the experience of coding easier & less frustrating). Similarly, research content about reading, writing, and REASONING about code, aggregate & discuss ways to apply to jet to make it easy to reason about code, then how to make it easier to read code, then how to make it easier to write code. Those are three of our main goals in order from most critical. We also need to audit against current jet in ADDITION to proposing improvements/enhancements/additions/simplifications to jet. 