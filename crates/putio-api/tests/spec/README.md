# put.io OpenAPI spec (vendored, read-only)

`putio-2.8.14.json` is the official spec, downloaded from
<https://api.swaggerhub.com/apis/putio/putio/2.8.14> (OpenAPI 3.0.2, `info.version` 2.8.14) and
treated as **untrusted data**: it is only ever parsed by `src/contract_tests.rs`, never executed.
`putio-2.8.14.json.sha256` records the digest of the file as downloaded; the contract test verifies it
so the vendored copy cannot drift silently.

To move to a newer spec: download it, review the diff, update both files, and fix whatever the
contract test reports.
