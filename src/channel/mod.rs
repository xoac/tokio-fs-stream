use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::path::PathBuf;

mod error;
mod fs_receiver;
mod fs_sender;

use fs_sender::UnboundedFileSender;

/// This channel will send all items through file system. It's recommended to use channel.
pub fn unbounded_file<T>(
    path: PathBuf,
) -> Result<
    (
        fs_sender::UnboundedFileSender<T>,
        fs_receiver::FileReciver<T>,
    ),
    io::Error,
>
where
    T: Serialize + DeserializeOwned,
{
    Ok((
        fs_sender::unbounded::<T>(&path)?,
        fs_receiver::new::<T>(path)?,
    ))
}

pub fn unordered_dir_fs<T>(
    dir_path: PathBuf,
    max_items_in_file: usize,
) -> Result<(fs_sender::DirSender<T>, fs_receiver::DirReciver<T>), io::Error>
where
    T: Serialize + DeserializeOwned,
{
    let dir_sender = fs_sender::new_dir_sender(dir_path.clone(), max_items_in_file)?;
    let dir_reciver = fs_receiver::new_dir_reciver(dir_path)?;
    Ok((dir_sender, dir_reciver))
}

use fs_sender::{new_send_all, SendAllUnorderedFs};
use futures::{Sink, Stream};

pub trait SinkFsExt: Sink {
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
