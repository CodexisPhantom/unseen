# Spike: Better Auth RS (issue #3, ADR 0006)

Throwaway code. Results are in `docs/adr/0006-authentication-library.md`.

- `src/auth_schema.rs`: output of `better-auth-rs generate --plugins api-key`.
- `src/main.rs`: server on port 7979 with the code for checks 3, 4 and 5.
- `client/test.mjs`: checks 2 and 3 through `better-auth/svelte` 1.7.6.

Linux (Docker):

```
docker build -t unseen-spike-auth .
bash run.sh
cd client && pnpm install && node test.mjs
```

Windows: set `OPENSSL_DIR`, `RUST_ICU_LINK_SEARCH_DIR` and
`RUST_ICU_MAJOR_VERSION_NUMBER=78`, add the OpenSSL and ICU `bin` folders to
`PATH`, then `cargo run`.
