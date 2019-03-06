use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::path::PathBuf;

mod error;
mod fs_receiver;
mod fs_sender;

use fs_receiver::{DirReciver, FileReciver};
use fs_sender::{DirSender, UnboundedFileSender};

/// Create a pair of UnboundedFileSender and FileReciver.
///
/// [UnboundedFileSender](struct.UnboundedFileSender.html) implements Sink to allow you easy serialize items on disk.
/// FileReciver implements Stream to easy deserlize items from file.
///
/// # Warning
/// This can cause a big I/O traffic. Don't use too often.
pub fn unbounded_file<T>(
    path: PathBuf,
) -> Result<(UnboundedFileSender<T>, FileReciver<T>), io::Error>
where
    T: Serialize + DeserializeOwned,
{
    Ok((
        fs_sender::unbounded::<T>(&path)?,
        fs_receiver::new::<T>(path)?,
    ))
}

/// Use dir as place to store files. It will be creating next file after last one is full.
pub fn unordered_dir_fs<T>(
    dir_path: PathBuf,
    max_items_in_file: usize,
) -> Result<(DirSender<T>, DirReciver<T>), io::Error>
where
    T: Serialize + DeserializeOwned,
{
    let dir_sender = fs_sender::new_dir_sender(dir_path.clone(), max_items_in_file)?;
    let dir_reciver = fs_receiver::new_dir_reciver(dir_path)?;
    Ok((dir_sender, dir_reciver))
}

use fs_sender::{new_send_all, SendAllUnorderedFs};
use futures::{Sink, Stream};

/// Extension trait for Sink that allow easy to use this library.
pub trait SinkFsExt: Sink {
    /// Use `dir_path` to save items from `stream` if `self` (Sink) is not ready. When it will be
    /// ready again items from file will be read.
    ///
    /// # Warning
    /// This use SendAllUnorderedFs so it can reorder items!
    ///
    /// # Notes
    /// It's useful to use it with futures-retry library. See `examples/file_unordered.rs`.
    fn send_all_fs_backpresure<U>(
        self,
        stream: U,
        dir_path: PathBuf,
    ) -> io::Result<SendAllUnorderedFs<Self, U>>
    where
        Self: Sized,
        U: Stream<Item = Self::SinkItem>,
        Self::SinkError: From<U::Error>,
        Self::SinkItem: Serialize + DeserializeOwned,
    {
        let (dir_sender, dir_reciver) = unordered_dir_fs(dir_path, 1000)?;
        Ok(new_send_all(self, stream, dir_sender, dir_reciver))
    }
}

impl<T> SinkFsExt for T where T: Sink {}
