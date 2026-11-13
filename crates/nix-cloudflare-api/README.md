# nix-cloudflare-api

> The Worker half of [nix-cloudflare](../..): a Nix binary cache on Cloudflare
> Workers and R2, written in Rust. It serves narinfo and NARs from R2 and signs
> uploads.

> [!WARNING]
> **The Worker has no authentication or authorization.** Anyone who can reach it
> can read the cache and upload to it, and the Worker signs what they upload
> with its key, so an open cache can be poisoned. Put it behind something that
> decides who gets in: see [Access control](#access-control).

## Deploy

1. **Create the signing key** and store it in [Secrets Store](https://developers.cloudflare.com/secrets-store/), so it never passes through your deploy tooling. Wrangler prompts for the value: paste the whole `<key-name>:<base64>` line.
   ```sh
   nix key generate-secret --key-name cache.example.com-1
   wrangler secrets-store secret create <store-id> --name nix-cloudflare-signing-key --scopes workers --remote
   ```
   Clients need the matching public key in `trusted-public-keys` (`nix key convert-secret-to-public`, or the `PublicKey:` line of `<cache-url>/nix-cache-info`).
2. **Decide how to protect it**: [Access control](#access-control). The Worker doesn't.
3. **Deploy** the released bundle with the [Terraform / OpenTofu module](../../deployment/terraform) (`//deployment/terraform?ref=<version>`). It downloads the release (`index.js` and `index_bg.wasm`, both required), creates the R2 bucket, and sets up the bindings below and the workers.dev URL (or an optional custom domain). To deploy a local build instead, run `worker-build --release` here and point the module's `bundle_dir` at this directory's `build/`.
4. **Check** that `<cache-url>/health/ready` returns `200`: the bindings were accepted, and the signing key can be read. An unbound bucket fails every request, this one too, as a `500`, and a signing key that can't be read or isn't valid answers `503`; the reasons are in Workers Logs. To check it on every plan and apply, see the module's [Checking it's ready](../../deployment/terraform#checking-its-ready).

`wrangler.toml` in this directory is for local development, not production.

## Bindings

| Binding | Type | Required | Description |
|---|---|---|---|
| `NIX_CLOUDFLARE_API_BUCKET` | R2 bucket | yes | Stores `.narinfo` and `.nar` objects. |
| `NIX_CLOUDFLARE_API_SECRET` | Secrets Store secret | conditional | `<key-name>:<base64>`, as emitted by `nix key generate-secret`. Required unless every uploader sends signed narinfo. Only a Secrets Store binding is taken: bound as a plain secret or var, the Worker refuses to start, so the key never passes through Terraform or a command line. |

## Access control

The Worker doesn't authenticate or authorize anyone. Every request is served,
reads and uploads alike, so protecting the cache is the deployment's job. You
need one of:

- **Cloudflare Access** in front of the Worker, as a
  [self-hosted application](https://developers.cloudflare.com/cloudflare-one/applications/configure-apps/self-hosted-public-app/)
  on its custom domain, with the policy you want. Access matches paths, not
  HTTP methods, and the cache reads and writes the same paths, so one policy
  covers both.
- **A gateway of your own**: a reverse proxy or another Worker that
  authenticates callers and forwards to this one, and decides who may read
  and who may write.

Either way, keep the Worker's own address out of reach: if its `workers.dev`
URL stays enabled, or anything can reach the origin directly, the gateway can
be walked around.

### Signing in from Nix

Nix sends credentials to a binary cache only from a netrc file (`netrc-file`
in `nix.conf`, or `--option netrc-file`), as HTTP Basic, and can't send other
headers. So a gateway has to take what Nix can send, or the client has to
authenticate for Nix.

[nix-vault](https://github.com/nixos-contrib/nix-vault) is the client for that:
`aws-vault` for Nix. It signs you in, with HTTP Basic or OAuth2 (device flow,
or authorization code with PKCE), keeps the credentials in your keychain, and
runs Nix with a session netrc and `nix.conf` that carry them. Point it at
your gateway, which checks the Basic password (a token it issued) the way you
decide.

What this project has not tried is Nix against Cloudflare Access: Access
doesn't take Basic credentials, so Nix can't answer its login on its own, and
no component here bridges the two.

CI that needs to push has the same choice: whatever your gateway accepts, in
the netrc, for the length of the job.

## HTTP API

The API is specified by the SDK's [OpenAPI document](../nix-cloudflare-sdk/openapi/nix/cache/v1/cachev1.yaml).
The Worker implements the server [generated from it](../nix-cloudflare-sdk), so
requests are checked against the document before a handler sees them. Uploads
must use the content type Nix sends: `text/x-nix-narinfo` for narinfo and
`application/x-nix-nar` for NARs.

| Method | Path | Description |
|---|---|---|
| `GET` | `/nix-cache-info` | Cache metadata (priority, etc.), and the signing key's public half as `PublicKey: <key-name>:<base64>` when `NIX_CLOUDFLARE_API_SECRET` is set. Nix ignores that line: it's there to copy into `trusted-public-keys`. |
| `GET` | `/<hash>.narinfo` | Narinfo for a store path. |
| `HEAD` | `/<hash>.narinfo` | Existence check for a narinfo (200 / 404). |
| `PUT` | `/<hash>.narinfo` | Upload a narinfo. |
| `POST` | `/` | Mass query: a newline-separated list of hashes in, the cached subset out. |
| `GET` | `/nar/<hash>.nar` | NAR archive bytes. |
| `HEAD` | `/nar/<hash>.nar` | Existence check for a NAR (200 / 404). |
| `PUT` | `/nar/<hash>.nar` | Upload a NAR archive. |
| `GET` | `/health/live` | `200` while the Worker is up and serving, and `500`, like every request, while its bucket is unbound. The SDK's `HealthHandler`; a deployment check, so not in the OpenAPI document. |
| `GET` | `/health/ready` | `200` when `/health/live` is and the signing key, if one is bound, can be read and is a valid key. `503` otherwise, with why in Workers Logs (`unready:`). It doesn't touch the bucket. |

**Errors** are JSON: `{ "error": "<code>", "message": "<reason>" }`. Nix prints the body of a failed upload, so the message says what to fix, except for `500`, whose details go only to the logs.

| Status | `error` | Means |
|---|---|---|
| `400` | `bad_request` | The narinfo or request is invalid, e.g. `narinfo is missing NarSize` |
| `404` | `not_found` | No such narinfo or NAR |
| `500` | `misconfigured` | The bindings or the signing key are invalid |
| `500` | `internal_error` | A stored object is unreadable |

A request the document doesn't allow is rejected before it reaches the Worker,
as `application/problem+json` with a `code`: `415` for an undeclared content
type, `413` for a body over 64 MiB, `400` or `422` for a malformed request.

**Validation and signing:** the Worker parses each uploaded narinfo, checks its
format, and requires its `StorePath` to match the request's hash. Every stored
narinfo carries a `Sig:`. If the uploader didn't sign and `NIX_CLOUDFLARE_API_SECRET`
is set, the Worker signs the upload itself. Otherwise the `PUT` returns `400`.

## Security

- **No access control.** See [Access control](#access-control): who may read
  and who may upload is decided in front of the Worker. An open cache's
  uploads are signed with the Worker's key, so only expose it to callers you
  trust to upload.
- **The signing key is the only secret.** It lives in Secrets Store, so it
  never passes through Terraform or CI, and is never logged.
- **Uploads are validated.** Each narinfo is parsed, its `StorePath` must match
  the hash, and it carries a signature.

## Limitations

- No authentication, authorization or revocation: see
  [Access control](#access-control).
- Compressed NARs (`.nar.xz` etc.) aren't supported: uploads must use
  `?compression=none`.
- NARs are held in memory on the way in and out, and an upload can be at most
  64 MiB (`413`). A Worker has 128 MB of memory.
- Secrets Store is in open beta.

## Development

The crate is a member of the Cargo workspace at the repo root. It builds the
SDK, which generates the API from its OpenAPI document, so a change to the
document shows up as a compile error here. Run everything inside the dev shell:

```bash
nix develop -c cargo test           # unit tests, from the repo root
nix develop -c worker-build --dev   # build the bundle into ./build (this directory)
nix develop -c wrangler dev         # serve locally (this directory)
```

To sign uploads locally, put a signing key in the local Secrets Store that
`wrangler.toml` binds; Wrangler prompts for the value, so it stays off the
command line. Without one, only narinfo the uploader signed can be stored.

```bash
nix key generate-secret --key-name local-dev   # a throwaway key: copy the line it prints
nix develop -c wrangler secrets-store secret create 00000000000000000000000000000000 --name signing-key --scopes workers   # paste it when prompted
```

The integration tests call the Worker under `wrangler dev` through the SDK's
client. They need no credentials. `tests/run.sh` starts a `wrangler dev` of their own, on port 8789
with its own local storage and a throwaway signing key, runs them, and stops
it, so one you're running is left alone. CI runs it too:

```bash
nix develop -c tests/run.sh                                  # this directory
```

To run them against a `wrangler dev` you've started instead, set
`NIX_CLOUDFLARE_API_URL` if it isn't on `http://127.0.0.1:8787`:

```bash
nix develop -c cargo test --features integration
```

## Dependencies

- [`nix-cloudflare-sdk`](../nix-cloudflare-sdk), the API's types, the server traits the Worker implements, and the narinfo format: parsing, validation and signing
- [`worker`](https://crates.io/crates/worker) and [`worker-macros`](https://crates.io/crates/worker-macros), the Cloudflare Workers Rust SDK, with [`axum`](https://crates.io/crates/axum), which serves the SDK's router
