# Wire forms

A record marked `#PublishedSchema` keeps its shape; `#Skip` and `#Flatten`
shape one field, and `#DenyUnknownFields` refuses a stray key.

```jet
#[Encode, Rename(.Camel)]
Reply: struct { ok: Bool }
```

C# code and a `#Encoded` word stay as written.
