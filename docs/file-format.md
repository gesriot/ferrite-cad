# Native document format

A `.fcad` document is a SQLite database tagged with application id `FCAD`.
It uses rollback journalling and full durability. The schema is created directly:
metadata, objects, dependencies, topology references, imported source bytes and
checkpoints. Object and topology payloads carry BLAKE3 integrity hashes.

Each object has one CBOR envelope containing `type` and a nested CBOR byte string
`payload`. Each supported type has one payload structure. Imported scenes store
source definition keys and durable placement identities. An unknown type or an
unreadable payload fails the entire opening operation with the object id and type;
opening a damaged document does not modify it.

A `.fcad-cache` sidecar is tagged `FCAC`. It uses WAL and normal durability, and
binds derived blobs to a document id, kernel build and input key. Unreadable or
mismatched caches are discarded and recreated. A cached entry whose bytes no
longer match their hash is dropped; a miss rebuilds geometry.

The document format is unstable. Change its schema and payloads directly and
regenerate affected fixtures. No migration, layout version or capability contract
is stored or negotiated.
