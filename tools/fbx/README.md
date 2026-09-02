# Production FBX writer gate inputs

`digests.tsv` records the SHA-256 of the two files
`crates/ferritecad-export/examples/fbx_gate_artefacts.rs` produces from the
scenes in `crates/ferritecad-export/tests/fbx_scene/mod.rs`.

Both scenes are pure arithmetic: no kernel, no document and no tessellator
takes part, so the bytes are a function of the writer alone. Comparing them
with these digests on Linux, macOS and Windows is what turns "the same scene
always produces the same bytes" from a claim about one machine into a measured
property of three.

If a change to the writer or to either scene changes the output, regenerate
with:

```sh
tools/check-fbx-writer.sh --record
```

and say in the commit what changed and why, because a digest that moves without
a reason is a writer that is no longer a function of its input.
