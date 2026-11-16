# ELI5 example patterns

Read this reference only when a concrete example helps explain a topic. Choose one example that demonstrates the mechanism and adapt it to the user's subject.

## Technical definition

Before:

> A cache is a high-speed data storage layer that stores a subset of data, typically transient in nature, so future requests are served faster than accessing the primary storage location.

After:

> A cache keeps a nearby copy of data you are likely to need again, so you can get it faster. For example, a browser saves a site's logo instead of downloading the same image on every visit. The saved copy can become outdated, so caches need rules for when to refresh or discard it.

## Networking

Before:

> DNS performs hierarchical, distributed resolution of domain names into IP addresses.

After:

> DNS is the internet's address book. You give it a name such as `example.com`, and it returns the numeric address computers use to reach that site. “Address book” is only an analogy: DNS is a distributed system of many servers, not one central list.

## Programming

Before:

> A closure is a function bundled with references to its lexical environment.

After:

> A closure is a function that remembers values from where it was created. If you create a function while `taxRate` is 0.2, that function can still use `taxRate` later, even after the surrounding code has finished. More precisely, it keeps access to the variables it captured, not necessarily frozen copies of their values.

## Probability

Before:

> A 20% probability does not imply the event occurs once in every five trials due to variance in finite samples.

After:

> A 20% chance means the event is expected about 20 times across many similar tries. It does not promise one success in each group of five. You could get no successes in five tries, or several; the results tend to approach 20% only over many tries.
