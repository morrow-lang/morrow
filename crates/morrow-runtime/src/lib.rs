//! Native services for compiled Morrow programs.
//!
//! Values cross the generated-code boundary through fixed C calling conventions;
//! allocation, ownership and service implementation belong to this Rust crate.
pub mod abi;
pub mod actors;
pub mod collections;
pub mod foreign;
pub mod io;
pub mod json;
pub mod json_codec;
pub mod managed;
pub mod memory;
pub mod process;
pub mod services;
pub mod strings;
pub mod tui;
pub mod values;
