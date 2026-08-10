# Sigstore bundle v0.3 verification fixture

These two files are the `cosign-v3-blob` interoperability fixture published in
the Apache-2.0-licensed `sigstore-verify` 0.11.0 crate from
`sigstore/sigstore-rust`. The artifact was signed by identity
`w.vollprecht@gmail.com` under issuer `https://github.com/login/oauth` and has
Rekor integrated time `1764787003`.

The repository test uses the production trust root embedded by the exact pinned
crate and requires the certificate chain, SCT, artifact signature, Rekor
checkpoint/inclusion proof/promise, exact identity, and exact issuer. It also
checks artifact, identity, and issuer mismatch. This is a compatibility vector,
not SQLite Capsule release evidence.
