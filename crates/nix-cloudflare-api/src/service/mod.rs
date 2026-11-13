//! Service implementations for the generated API traits.
//!
//! The Worker's configuration, the one place its bindings are read, is in
//! [`config`]. The service types and the trait impls are in [`handler`].
//! Nothing here authenticates or authorizes: the cache is open, and whoever
//! deploys it decides how to protect it.
//!
//! The health endpoints are the SDK's `HealthHandler`, merged beside them in
//! the crate root: they're a deployment check, not part of the API, so the
//! spec doesn't declare them. Readiness asks [`health`]'s check of the
//! config, which reads the signing key.

pub mod config;
pub mod handler;
pub mod health;
