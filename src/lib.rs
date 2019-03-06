//! Work in progress.
//!
//! This create is aiming to prevent from losing data by saving them on disk and read automatically
//! when program is restarted.
//!
//! The idea is to start saving on disk when sink return Async::NotReady(item). `item` has to impl
//! Serialize and Deserialize. At this moment [async
//! bincode](https://crates.io/crates/async-bincode) is used under hood. It's designed to be used
//! with [futures-retry](https://crates.io/crates/futures-retry) Sink implementation.
pub mod channel;

// TODO before #![deny(missing_docs)]

pub use channel::SinkFsExt;
