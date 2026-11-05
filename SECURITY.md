# Security policy

## Reporting
Please report vulnerabilities privately through GitHub's "Report a vulnerability" (Security tab). Do not open a public issue.

## Design rules
- The put.io token lives only in the OS secret store (Keychain / Credential Manager / Secret Service). It is never written to a settings file, log, URL or command line.
- TLS is rustls only; `openssl` and `native-tls` are banned in `deny.toml`.
- Only crates.io dependencies, `Cargo.lock` is committed, all CI uses `--locked`.
- Every GitHub Action is pinned to a full commit SHA.
- Anything arriving from the OS (magnet URIs, file paths, command-line arguments, IPC frames) is validated as hostile input.

## Verifying a release
Releases are built by GitHub Actions from a signed tag. Each artifact has a build-provenance attestation:

    gh attestation verify <artifact> --repo nicoSWD/putio-adder-rs

`SHA256SUMS` and a CycloneDX SBOM are published with every release.
