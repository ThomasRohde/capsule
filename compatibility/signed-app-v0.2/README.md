# Signed application test vector

This directory freezes the `org.sqlite-capsule.signed-app/0.2` canonical stream for the current capsule format. The fixture covers non-ASCII text, binary data, JSON canonicalisation, ordered compound endpoint steps, and excluded mutable tables.

The seed is test-only material and confers no publisher trust.

Run both implementations with:

```powershell
python tools/check_signed_app_vectors.py
cargo test --manifest-path native/Cargo.toml -p sqlite-capsule-crypto rust_matches_the_independent_v02_golden_vector
```

