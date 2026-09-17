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


Building A Programming Language Playlist -> Mine entire playlist
https://youtube.com/playlist?list=PLET80Nvdg3mg&si=we8LZwmFSPfQnOkA
> What can we learn from this playlist & apply to jet? What strengths, weaknesses, opportunities, and threats do we have. What can we adopt from these videos at a strategic, operational, and tactical level? What features,functions/methods, ideas, structures, components, libraries, keywords, facets, etc. can we learn from this playlist & the experience of building a language from scratch?

~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
Have Astra research videos/articles/blog posts/forum posts/etc. to find perspectives about why learning to code is hard for people. Use this to funnel into a discussion about how to address these issues from the language level. How can these issues be prevented or minimized by jet? What language level support can we provide to make learning easier (not by literally teaching, but making the experience of coding easier & less frustrating). Similarly, research content about reading, writing, and REASONING about code, aggregate & discuss ways to apply to jet to make it easy to reason about code, then how to make it easier to read code, then how to make it easier to write code. Those are three of our main goals in order from most critical. We also need to audit against current jet in ADDITION to proposing improvements/enhancements/additions/simplifications to jet. 