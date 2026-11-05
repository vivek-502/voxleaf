//! VoxLeaf's shared domain types and the HTTP surface that serves them.
//!
//! The web app was a Next.js project whose route handlers returned either
//! `{ data }` or `{ error: { code, message } }`. This crate reproduces that
//! contract exactly — same paths, same payloads, same status codes — so the
//! ported React UI needs almost no changes.

pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod ids;
pub mod models;
pub mod repo;
pub mod services;
pub mod storage;

pub use error::{ApiError, ApiResult};
