# CycloneDX JSON schema, as checked in

The Rust SBOM fragment is validated against the real published CycloneDX
schema, not against a partial parser written here. That schema is three files,
because `bom-1.5.schema.json` refers to the other two.

## What is here, and where each file came from

All three were taken from the `CycloneDX/specification` repository at tag
`1.5`, commit `c320fc0f0b46873864927d9d5684eea7ba439728`:

| file | upstream path | SHA-256 |
| --- | --- | --- |
| `bom-1.5.schema.json` | `schema/bom-1.5.schema.json` | `067f7824b08653839ea050ae9e09ca48375eadc2652b0e2a299476e7db90335b` |
| `spdx.schema.json` | `schema/spdx.schema.json` | `4f6e2b05c05d26a4f2dc5879fbc2fca94b0a28db46289d0c51345621b71cfbfc` |
| `jsf-0.82.schema.json` | `schema/jsf-0.82.schema.json` | `8bae002c25e723db7ee1f26afde680ae1a2b1a8f6b4b4b0fd65dc3becb090aae` |

The raw URL of each is
`https://raw.githubusercontent.com/CycloneDX/specification/c320fc0f0b46873864927d9d5684eea7ba439728/schema/<file>`.

The commit, the tag and all three digests are recorded in
[`../pin.env`](../pin.env), which is where the scripts read them from. The
digests are checked on every run, so a file edited in place is a refusal
rather than a quietly different definition of "valid".

## Why tag `1.5` and not `1.5.1`

Both tags publish a file called `bom-1.5.schema.json` and they are not
identical. Diffed before choosing: `1.5.1` drops `version` from the top-level
`required` list and rewrites two `description` strings that point at the
package-url version-range specification. Nothing else differs.

Tag `1.5` is therefore the stricter of the two, and the fragment emits
`version` regardless, so taking the stricter schema costs nothing and refuses
one more thing.

## Why the files are copied rather than fetched

`bom-1.5.schema.json` declares
`"$id": "http://cyclonedx.org/schema/bom-1.5.schema.json"` and refers to its
two companions by relative name. A validator resolves those references against
the `$id`, which means it goes to the network. Measured: with the two
companion files deleted from the directory, validation of an instance carrying
a bogus SPDX licence id was still correctly refused, so the SPDX enumeration
had been fetched rather than read from disk.

An offline gate cannot be built on that. `tools/check-rust-sbom.sh` therefore
copies these three files into a temporary directory, verifies each digest, and
deletes the one top-level `$id` from its copy of `bom-1.5.schema.json`. With
no `$id`, the base is the file the schema was read from, and the two relative
references resolve to the copies sitting beside it.

That this is real was measured the same way round: with the companions removed
from the directory, validation of the same instance now fails with
`Resource 'file://.../spdx.schema.json' is not present in a registry and
retrieving it failed` instead of passing. Nothing is fetched.

Deleting the `$id` is the whole edit, and it is made to a copy. The bytes that
decide what is valid are the digest-checked bytes above.
