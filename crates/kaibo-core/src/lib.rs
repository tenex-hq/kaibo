#![forbid(unsafe_code)]

//! Core library for kaibo.
//!
//! This crate holds all logic: config resolution, the frontmatter model, and
//! the output and error contracts shared by every verb. It never prints to a
//! stream and never calls `std::process::exit` - the binary crate (`kaibo`)
//! is the only place either of those happens. A second face (an MCP server,
//! a hosted API handler) links this crate directly instead of reimplementing
//! any of it.

pub mod config;
pub mod error;
pub mod explain;
pub mod frontmatter;
pub mod output;
