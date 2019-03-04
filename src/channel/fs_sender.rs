use async_bincode::{AsyncBincodeWriter, AsyncDestination, SyncDestination};
// use bincode::Error;
use super::error::{self, Error};
use super::fs_receiver::DirReciver;
use custom_error::{add_type_bounds, custom_error};
use futures::prelude::*;
use futures::{stream::Fuse, try_ready};
use log::trace;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use tokio_fs::File;

/// Unbounded Sender through file that will be saving all item until fs limit.
pub struct UnboundedFileSender<T> {
    writer: AsyncBincodeWriter<File, T, AsyncDestination>,
    closing: ClosingFile,
}

#[derive(PartialEq, Eq)]
enum ClosingFile {
    None,
    PollComplete,
    ReadPermision,
    WritePerminison(std::fs::Permissions),
    Writer,
}

/// Create new unbounded sender file in path.
///
/// # Notes
/// It will append to file if no exist.
///
/// # Warning
/// It's logical error to use file that already exist on file system with unknow body.
pub fn unbounded<T>(path: &PathBuf) -> io::Result<UnboundedFileSender<T>> {
    let write_fd_std = dbg!(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?);

    let write_file = File::from_std(write_fd_std);
    let writer: AsyncBincodeWriter<File, T, SyncDestination> = write_file.into();
    Ok(UnboundedFileSender {
        writer: writer.for_async(),
        closing: ClosingFile::None,
    })
}

impl<T> Sink for UnboundedFileSender<T>
where
    T: Serialize,
{
    /// The type of value that the sink accepts.
    type SinkItem = T;

    /// The type of value produced by the sink when an error occurs.
    type SinkError = bincode::Error;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.writer.start_send(item)
        //self.writer.start_send(Some(item)).map(|async_sink| async_sink.map(|opt_item| opt_item.unwrap()))
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.writer.poll_complete()
    }

    //
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        // TODO we need here some state to know witch part we should poll in next call.
        // We should also notice when we closed succesfully to panic when start_send or
        // poll_complete is called after close.


        if self.closing == ClosingFile::None {
            self.closing = ClosingFile::PollComplete;
        }

        loop {
            match self.closing {
                ClosingFile::None => unreachable!(),
                ClosingFile::PollComplete => {
                    trace!("Close is called -> PollComplete");
                    try_ready!(self.poll_complete());
                    self.closing = ClosingFile::ReadPermision;
                }
                ClosingFile::ReadPermision => {
                    trace!("Close is called -> ReadPermision");
                    let mut perms = try_ready!(self.writer.get_mut().poll_metadata()).permissions();
                    perms.set_readonly(true);
                    self.closing = ClosingFile::WritePerminison(perms);
                }
                ClosingFile::WritePerminison(ref perms) => {
                    trace!("Close is called -> WritePerminison");
                    try_ready!(self.writer.get_mut().poll_set_permissions(perms.clone()));
                    self.closing = ClosingFile::Writer;
                }
                ClosingFile::Writer => {
                    trace!("Close is called -> Writer");
                    return self.writer.close();
                }
            }
        }
    }
}

enum WrapError<T> {
    MaxItems(T),
    BinCodeError(bincode::Error),
}

impl<T> From<bincode::Error> for WrapError<T> {
    fn from(oth: bincode::Error) -> Self {
        WrapError::BinCodeError(oth)
    }
}

struct FileSender<T> {
    file: UnboundedFileSender<T>,
    number_of_items: usize,
    max_number_of_items: usize,
}

fn new_file_sender<T>(path: PathBuf, max_number_of_items: usize) -> io::Result<FileSender<T>> {
    let max_number_of_items = if max_number_of_items == 0 {
        usize::max_value()
    } else {
        max_number_of_items
    };

    let number_of_items = if path.exists() {
        0
    // TODO read how many items are in file and
    } else {
        0
    };

    let file = unbounded(&path)?;

    Ok(FileSender {
        file,
        number_of_items,
        max_number_of_items,
    })
}

impl<T> Sink for FileSender<T>
where
    T: Serialize,
{
    /// The type of value that the sink accepts.
    type SinkItem = T;

    /// The type of value produced by the sink when an error occurs.
    type SinkError = WrapError<T>;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if self.max_number_of_items <= self.number_of_items {
            return Err(WrapError::MaxItems(item));
        }

        Ok(match self.file.start_send(item)? {
            AsyncSink::Ready => {
                self.number_of_items += 1;
                AsyncSink::Ready
            }
            not_ready => not_ready,
        })
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.file.poll_complete().map_err(WrapError::from)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.file.close().map_err(WrapError::from)
    }
}

pub struct DirSender<T> {
    dir_path: PathBuf,
    file: FileSender<T>,
    next_file_index: u32,
    max_number_of_items: usize,
}

pub fn new_dir_sender<T>(
    dir_path: PathBuf,
    max_number_of_items: usize,
) -> io::Result<DirSender<T>> {
    if !dir_path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Path {:?} dosen't represent dir", dir_path),
        ));
    }

    let next_file_index = 0; //TODO powinno byc sprawdzone czy nie ma juz plikow w katalogu.

    let mut file_path = dir_path.clone();
    file_path.push(next_file_index.to_string());

    let max_number_of_items = if max_number_of_items == 0 {
        usize::max_value()
    } else {
        max_number_of_items
    };

    Ok(DirSender {
        dir_path,
        file: new_file_sender(file_path, max_number_of_items)?,
        next_file_index: next_file_index + 1,
        max_number_of_items,
    })
}

impl<T> DirSender<T> {
    fn next_path(&self) -> PathBuf {
        let mut path = self.dir_path.clone();
        path.push(format!("{}", self.next_file_index));
        path
    }
}

impl<T> Sink for DirSender<T>
where
    T: Serialize,
{
    /// The type of value that the sink accepts.
    type SinkItem = T;

    /// The type of value produced by the sink when an error occurs.
    type SinkError = Error;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.file.start_send(item) {
            Err(err) => match err {
                WrapError::MaxItems(item) => {
                    let next_path = self.next_path();
                    let file = new_file_sender(next_path, self.max_number_of_items)?;
                    self.file = file;
                    self.start_send(item)
                }
                WrapError::BinCodeError(err) => return Err(err.into()),
            },
            Ok(ok) => return Ok(ok),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.file.poll_complete().map_err(|err| match err {
            WrapError::MaxItems(_) => unreachable!(),
            WrapError::BinCodeError(err) => Error::from(err),
        })
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.file.close().map_err(|err| match err {
            WrapError::MaxItems(_) => unreachable!(),
            WrapError::BinCodeError(err) => Error::from(err),
        })
    }
}

pub fn new_send_all<T, U>(
    sink: T,
    stream: U,
    dir_sender: DirSender<T::SinkItem>,
    dir_reciver: DirReciver<T::SinkItem>,
) -> SendAllUnorderedFs<T, U>
where
    T: Sink,
    U: Stream<Item = T::SinkItem>,
    T::SinkError: From<U::Error>,
    T::SinkItem: Serialize + DeserializeOwned,
{
    SendAllUnorderedFs {
        sink: Some(sink),
        dir_sender,
        dir_reciver: dir_reciver.fuse(),
        stream: Some(stream.fuse()),
        buffered: None,
        stream_closed: Closing::Working,
        check_fs_required: true,
    }
}

pub struct SendAllUnorderedFs<T: Sink, U> {
    sink: Option<T>,
    dir_sender: DirSender<T::SinkItem>,
    dir_reciver: Fuse<DirReciver<T::SinkItem>>,
    // TODO we should guarantee that stream will not panic when called poll after returned None.
    stream: Option<Fuse<U>>,
    buffered: Option<T::SinkItem>, // item, ktory nie mogl zostac odrzucony
    stream_closed: Closing,
    check_fs_required: bool,
}

custom_error! { pub SendAllFsErr<T>
    StoreError { source: Error } = "Error ocurred during storage items on fs",
    Custom{ inner: T } = "Custom error occured",
}

fn from_custom_err<T>(oth: T) -> SendAllFsErr<T> {
    SendAllFsErr::Custom { inner: oth }
}

enum State {
    PollComplateRequired,
    Sended,
}

#[derive(PartialEq, Eq)]
enum Closing {
    Working,
    DirSender,
    ReadingFs,
    Sink,
    Return,
}

impl<T, U> SendAllUnorderedFs<T, U>
where
    T: Sink,
    U: Stream<Item = T::SinkItem>,
    T::SinkError: From<U::Error>,
    T::SinkItem: Serialize,
    for<'de> T::SinkItem: Serialize + Deserialize<'de>,
{
    fn sink_mut(&mut self) -> &mut T {
        self.sink
            .as_mut()
            .take()
            .expect("Attempted to poll SendAllUnorderedFs after completion")
    }

    fn stream_mut(&mut self) -> &mut Fuse<U> {
        self.stream
            .as_mut()
            .take()
            .expect("Attempted to poll SendAllUnorderedFs after completion")
    }

    fn try_send_to_sink(&mut self, item: T::SinkItem) -> Poll<(), SendAllFsErr<T::SinkError>> {
        debug_assert!(self.buffered.is_none());
        if let AsyncSink::NotReady(item) =
            self.sink_mut().start_send(item).map_err(from_custom_err)?
        {
            self.buffered = Some(item);
            return Ok(Async::NotReady);
        }
        Ok(Async::Ready(()))
    }

    fn try_send_to_sink_or_dir(
        &mut self,
        item: T::SinkItem,
    ) -> Poll<(), SendAllFsErr<T::SinkError>> {
        //TODO this can change order of items.
        debug_assert!(self.buffered.is_none());
        if let AsyncSink::NotReady(item) =
            self.sink_mut().start_send(item).map_err(from_custom_err)?
        {
            if let AsyncSink::NotReady(item) = self.dir_sender.start_send(item)? {
                self.buffered = Some(item);
                return Ok(Async::NotReady);
            } else {
                trace!("try_send_to_sink_or_dir -> item addted to dir!");
                self.check_fs_required = true;
            }
        } else {
            trace!("try_send_to_sink_or_dir -> item addted to sink!");
        }
        Ok(Async::Ready(()))
    }

    fn try_get_item_stream(&mut self) -> Poll<Option<T::SinkItem>, SendAllFsErr<T::SinkError>> {
        if let Some(item) = self.buffered.take() {
            return Ok(Async::Ready(Some(item)));
        }

        self.stream_mut()
            .poll()
            .map_err(T::SinkError::from)
            .map_err(from_custom_err)
    }

    fn try_get_item_fs(&mut self) -> Poll<Option<T::SinkItem>, SendAllFsErr<T::SinkError>> {
        if let Some(item) = self.buffered.take() {
            return Ok(Async::Ready(Some(item)));
        }

        self.dir_reciver.poll().map_err(SendAllFsErr::from)
    }

    fn try_get_item(&mut self) -> Poll<Option<T::SinkItem>, SendAllFsErr<T::SinkError>> {
        if let Some(item) = self.buffered.take() {
            return Ok(Async::Ready(Some(item)));
        }

        match self.dir_reciver.poll()? {
            Async::Ready(Some(item)) => return Ok(Async::Ready(Some(item))),
            Async::Ready(None) => (), // dir is close but stream can be still open.
            Async::NotReady => (),
        };

        self.stream_mut()
            .poll()
            .map_err(T::SinkError::from)
            .map_err(from_custom_err)
    }

    /// Send all items from stream to fs_storage. It should be called only when fs_storage is not
    /// empty.
    fn fill_fs_sink(&mut self) -> Poll<(), SendAllFsErr<T::SinkError>> {
        loop {
            if let Some(item) = try_ready!(self.try_get_item_stream()) {
                if let AsyncSink::NotReady(item) = self.dir_sender.start_send(item)? {
                    trace!("\t \t fill_fs_sink -> NotReady");
                    self.buffered = Some(item);
                    return Ok(Async::NotReady);
                }
            }
        }
    }

    fn read_fs_and_fill_sink(&mut self) -> Poll<(), SendAllFsErr<T::SinkError>> {
        loop {
            //FIXME this probably can be infinite loop in cerain situation;
            match try_ready!(self.try_get_item_fs()) {
                Some(item) => try_ready!(self.try_send_to_sink(item)),
                None => return Ok(Async::Ready(())),
            };
        }
    }

    fn try_sink_or_dir_poll_complete(&mut self) -> Poll<(), SendAllFsErr<T::SinkError>> {
        let sink_res = self.sink_mut().poll_complete().map_err(from_custom_err)?;
        let dir_res = self.dir_sender.poll_complete()?;
        if sink_res.is_ready() && dir_res.is_ready() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn take_result(&mut self) -> Poll<(T, U), SendAllFsErr<T::SinkError>> {
        Ok(Async::Ready((
            self.sink.take().expect("Calling after resolve is error!"),
            self.stream
                .take()
                .expect("Calling after resolve is error!")
                .into_inner(),
        )))
    }
}

impl<T, U> Future for SendAllUnorderedFs<T, U>
where
    T: Sink,
    U: Stream<Item = T::SinkItem>,
    T::SinkError: From<U::Error>,
    T::SinkItem: Serialize + DeserializeOwned,
{
    type Item = (T, U);
    type Error = SendAllFsErr<T::SinkError>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            trace!("SendAllUnorderedFs -> poll");
            match self.stream_closed {
                Closing::Working => (),
                Closing::DirSender => {
                    trace!("Poll close for dir sender is called");
                    try_ready!(self.dir_sender.close());
                    self.stream_closed = Closing::ReadingFs;
                }
                Closing::ReadingFs => {
                    trace!("Stream is closed. Reading only fs_receiver");
                    try_ready!(self.read_fs_and_fill_sink());
                    self.stream_closed = Closing::Sink;
                }
                Closing::Sink => {
                    trace!("Poll complet for sink through close is called()");
                    try_ready!(self.sink_mut().close().map_err(from_custom_err));
                    self.stream_closed = Closing::Return;
                }
                Closing::Return => {
                    return self.take_result();
                }
            }

            if Closing::Working != self.stream_closed {
                continue;
            }

            match self.try_get_item()? {
                Async::Ready(Some(item)) => {
                    try_ready!(self.try_send_to_sink_or_dir(item));
                }
                Async::Ready(None) => self.stream_closed = Closing::DirSender,
                Async::NotReady => {
                    trace!("Stream is not ready!");
                    dbg!(try_ready!(self.try_sink_or_dir_poll_complete()));
                }
            }
        }
    }
}
