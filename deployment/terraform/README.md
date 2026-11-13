# nix-cloudflare Terraform module

> The Terraform / OpenTofu half of [nix-cloudflare](../..): deploys the released
> Worker bundle to Cloudflare Workers with its R2 bucket, bindings and a
> workers.dev URL (or, optionally, a custom domain). No `wrangler` or local
> build is needed.
>
> The cache it deploys is **open**: it has no authentication or authorization,
> so anyone who can reach the URL can read and upload. Put Cloudflare Access or
> a gateway of your own in front of it; see
> [Protecting the cache](#protecting-the-cache).

```hcl
module "nix_cloudflare_api" {
  source = "git::https://github.com/nixos-contrib/nix-cloudflare.git//deployment/terraform?ref=v0.12.0" # x-release-please-version

  account_id  = var.account_id
  hostname    = "nix-cloudflare-api.example.workers.dev"
  bucket_name = "nix-cache"

  signing_key_secret = {
    secret_store_id = var.secret_store_id
    secret_name     = "nix-cloudflare-signing-key"
  }
}

output "nix_cloudflare_api_url" {
  value = module.nix_cloudflare_api.url
}
```

The module is released with the Worker from the same tag, and by default
deploys the Worker bundle of the release its `ref` points to.

## Prerequisites

- Terraform or OpenTofu >= 1.9.
- A narinfo signing key in [Secrets Store](https://developers.cloudflare.com/secrets-store/) (open beta), unless every uploader sends signed narinfo. See [Usage](#usage).
- An API token for deploying, exported as `CLOUDFLARE_API_TOKEN`, with:
  - **Account → Workers Scripts: Edit**
  - **Account → Workers R2 Storage: Edit**
  - **Account → Secrets Store: Edit**, to bind the signing key
  - **Zone → Workers Routes: Edit** on the cache's zone, only for a custom domain
- (Optional) `GITHUB_TOKEN` if you hit anonymous GitHub API rate limits while downloading the release.

## Usage

```sh
# Generate the signing key and store it once. Wrangler prompts for the value:
# paste the whole <key-name>:<base64> line.
nix key generate-secret --key-name cache.example.com-1
wrangler secrets-store store list --remote     # note the store ID
wrangler secrets-store secret create <store-id> --name nix-cloudflare-signing-key --scopes workers --remote

export CLOUDFLARE_API_TOKEN=...
tofu init
tofu apply
curl -fsS "$(tofu output -raw nix_cloudflare_api_url)/health/ready"   # 500 if a binding is wrong, 503 if the key can't be read
```

Terraform only references the secret by store ID and name. The key never
enters Terraform state or the plan. Clients need the matching public key in
`trusted-public-keys`: get it with
`nix key convert-secret-to-public < secret-key-file`.

Then [protect the cache](#protecting-the-cache), and push with
`nix copy --to '<url>?compression=none'`: the cache stores uncompressed NARs.

### Protecting the cache

The Worker doesn't authenticate anyone: whoever can reach `url` can read the
cache and write to it. That is the deployment's job, not the Worker's. Either:

- **Cloudflare Access** in front of the Worker, as a self-hosted application on
  its hostname (a custom domain, not a `workers.dev` one) with the policy you
  want. The Worker is untouched; Terraform for the Access application sits
  beside this module, not in it.
- **A gateway of your own**, a reverse proxy or another Worker that
  authenticates callers and forwards to this one.

Nix sends credentials only as HTTP Basic from a netrc file, so a gateway
that wants anything else, such as an Access login, needs a client that
authenticates for it. [nix-vault](https://github.com/nixos-contrib/nix-vault)
signs you in (HTTP Basic, or OAuth2 with a device or browser flow) and hands
Nix a session netrc for the run. The Worker's
[Access control](../../crates/nix-cloudflare-api/README.md#access-control)
section has the details.

### Checking it's ready

The module doesn't check the cache itself. To have every plan and apply warn
when it isn't ready, add
[terraform-http-check](https://github.com/tf-contrib/terraform-http-check)
beside it:

```hcl
module "nix_cloudflare_api_ready" {
  source = "git::https://github.com/tf-contrib/terraform-http-check.git?ref=v0.1.0"

  url  = "${module.nix_cloudflare_api.url}/health/ready"
  hint = "503 if the signing key can't be read or isn't valid, 500 if a binding is wrong. Workers Logs says why (unready: or misconfigured:)."
}
```

A key Secrets Store won't hand over then shows up in your apply, not on the
first upload. It's a warning, never an error, and gives up within about 25s on
a cache it can't reach. Leave it out where plans can't reach the cache, such as
runners whose egress is allowlisted.

## Inputs

| Variable | Required | Default | Description |
|---|---|---|---|
| `account_id` | yes | | Cloudflare account ID. |
| `hostname` | yes | | `<worker_name>.<subdomain>.workers.dev`, or a custom domain (needs `zone_id`). |
| `zone_id` | for a custom domain | `null` | Zone that holds a custom-domain `hostname`. |
| `bucket_name` | yes | | R2 bucket to create for `.narinfo` and `.nar` objects. |
| `expire_after_days` | no | `45` | Delete objects this many days after upload. `null` keeps them. |
| `signing_key_secret` | no | `null` | `{ secret_store_id, secret_name }` of the signing key. `null`: uploaders must sign. |
| `release_tag` | no | this module's release | Release to deploy, e.g. `v1.2.3`, or `"latest"`. |
| `bundle_dir` | no | `null` | A local `worker-build --release` output directory to deploy instead of a release. |
| `worker_name` | no | `nix-cloudflare-api` | Cloudflare Worker script name. |
| `worker_compatibility_date` | no | `2026-05-14` | Workers runtime compatibility date. |

## Outputs

| Output | Description |
|---|---|
| `url` | The cache URL, for `substituters` and `nix copy --to`. |
| `worker_name` | Deployed Worker script name. |
| `bucket_name` | R2 bucket backing the cache. |
| `release_tag` | Release that was deployed, or `"local"` for `bundle_dir`. |
| `worker_modules_sha256` | SHA-256 of each module the Worker version uploads, `index.js` and `index_bg.wasm`, of its base64 content. The plan shows the modules themselves only as `(sensitive value)`: this says which one changed. |

## Upgrading

Bump the `ref` in `source` and run `tofu apply`. The module downloads that
release's bundle, uploads a new Worker version and shifts all traffic to it.

## Migrating from `examples/terraform`

The example was a root module; this is a child module. To keep the existing
Worker and bucket, add `moved` blocks next to the `module` block before the
first `apply`:

```hcl
moved {
  from = cloudflare_worker.nix_cache
  to   = module.nix_cloudflare_api.cloudflare_worker.this
}
moved {
  from = cloudflare_worker_version.nix_cache
  to   = module.nix_cloudflare_api.cloudflare_worker_version.this
}
moved {
  from = cloudflare_workers_deployment.nix_cache
  to   = module.nix_cloudflare_api.cloudflare_workers_deployment.this
}
moved {
  from = cloudflare_r2_bucket.nix
  to   = module.nix_cloudflare_api.cloudflare_r2_bucket.this
}
moved {
  from = cloudflare_r2_bucket_lifecycle.nix
  to   = module.nix_cloudflare_api.cloudflare_r2_bucket_lifecycle.this[0]
}
```

- `r2_bucket_name` is now `bucket_name`, and `hostname` is new.
- `nix_secret` is gone: put the key in Secrets Store and set `signing_key_secret`. It then no longer sits in your Terraform state; remove the old value from any `terraform.tfvars`.
- The bindings are named `NIX_CLOUDFLARE_API_*`; the module sets them.

## Development

```sh
tofu init -backend=false
tofu test        # mocked providers: no credentials or network needed
```
