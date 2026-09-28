# Unsigned local fixture

This fixture exercises the explicit `--allow-unofficial` local-source path.
It is not a production release: the manifest and artifact intentionally have
no `.sig.json` sidecars. The client must reject this tree by default and must
never accept it through the default `https://dl.jet-lang.dev` endpoint.
The manifest contains versioned entries for both x86_64 and aarch64 Linux
targets, each with its own digest and artifact path.
