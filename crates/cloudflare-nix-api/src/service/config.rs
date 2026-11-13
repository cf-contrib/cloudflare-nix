//! The Worker's configuration, read from its bindings.
//!
//! [`Config::from_env`] is the only place that reads `Env`: the crate root
//! builds one per request and hands it to the handler and the health check,
//! which take what they need from it.
//!
//! An unbound bucket, or a signing key that isn't a Secrets Store binding,
//! fails it, and the Worker serves nothing until it's fixed. The signing
//! key's value is read only when it's needed, since reading it is async: to
//! sign an upload, for `/nix-cache-info`'s public key, and by the readiness
//! [`health`](super::health) check. A rotated one takes effect on the next.

use worker::{Bucket, Env, Error, SecretStore, js_sys, send::SendWrapper, wasm_bindgen::JsValue};

/// The binding of the R2 bucket every narinfo and NAR is stored in.
pub const BUCKET_KEY: &str = "CLOUDFLARE_NIX_API_BUCKET";

/// The binding of the narinfo signing key, `<key-name>:<base64>`. Optional:
/// without it, only narinfo the uploader signed can be stored.
pub const SECRET_KEY: &str = "CLOUDFLARE_NIX_API_SECRET";

/// What the Worker is configured with, from its bindings.
pub struct Config {
    /// `CLOUDFLARE_NIX_API_BUCKET`.
    bucket: BucketConfig,
    /// `CLOUDFLARE_NIX_API_SECRET`'s binding, not yet its value. None signs
    /// nothing.
    signing_key: Option<Secret>,
}

impl Config {
    /// Reads the Worker's bindings.
    ///
    /// # Errors
    ///
    /// When `CLOUDFLARE_NIX_API_BUCKET` isn't bound to an R2 bucket,
    /// or `CLOUDFLARE_NIX_API_SECRET` is bound but not from Secrets Store.
    pub fn from_env(env: &Env) -> worker::Result<Self> {
        Ok(Self {
            bucket: BucketConfig::from_env(env)?,
            signing_key: Secret::from_env(env, SECRET_KEY)?,
        })
    }

    /// The bucket every narinfo and NAR is stored in.
    pub fn bucket(&self) -> &Bucket {
        &self.bucket.0
    }

    /// Reads the narinfo signing key, `<key-name>:<base64>`, or `None` if
    /// none is bound.
    pub async fn signing_key(&self) -> worker::Result<Option<String>> {
        match &self.signing_key {
            Some(secret) => secret.read().await.map(Some),
            None => Ok(None),
        }
    }
}

/// The R2 bucket every narinfo and NAR is stored in. A `Bucket` holds a
/// JavaScript value, so it isn't `Send`; a Worker is single-threaded, so the
/// wrapper asserts it.
struct BucketConfig(SendWrapper<Bucket>);

impl BucketConfig {
    fn from_env(env: &Env) -> worker::Result<Self> {
        let bucket = env.bucket(BUCKET_KEY).map_err(|err| {
            Error::RustError(format!("{BUCKET_KEY} isn't bound to an R2 bucket: {err}"))
        })?;
        Ok(Self(SendWrapper::new(bucket)))
    }
}

/// A secret in Secrets Store, read when it's used.
///
/// Bound as anything else, a plain Worker secret or a var say, it's refused
/// rather than taken as a weaker setup: a Secrets Store secret never passes
/// through Terraform, or a command line.
struct Secret {
    key: &'static str,
    store: SecretStore,
}

impl Secret {
    /// The secret bound as `key`, or `None` if nothing is.
    fn from_env(env: &Env, key: &'static str) -> worker::Result<Option<Self>> {
        if let Ok(store) = env.secret_store(key) {
            return Ok(Some(Self { key, store }));
        }
        let bound = js_sys::Reflect::get(env.as_ref(), &JsValue::from_str(key))
            .is_ok_and(|binding| !binding.is_undefined());
        if bound {
            return Err(Error::RustError(format!(
                "{key} must be a Secrets Store binding"
            )));
        }
        Ok(None)
    }

    async fn read(&self) -> worker::Result<String> {
        let key = self.key;
        match self.store.get().await {
            Ok(Some(value)) if !value.is_empty() => Ok(value),
            Ok(_) => Err(Error::RustError(format!("{key} is empty"))),
            Err(err) => Err(Error::RustError(format!("{key} can't be read: {err}"))),
        }
    }
}
