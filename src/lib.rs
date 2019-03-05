//! The idea of this crate is to prevent losing data when ouer sink end out of ouer crate and can
//! be unaccessible.
//!
//! So we will be implementing sink that accept a item T and will try pass it further but when
//! error occured it will save it on disk instead of returning error to sb who send us this item.
//!
//! There should be also posibility to save every item on disk before sent it futher and spcify how
//! many items should be handled in ram.
pub mod channel;
//mod stream;


// TODO before #![deny(missing_docs)]

pub use {
    channel::SinkFsExt,
};


