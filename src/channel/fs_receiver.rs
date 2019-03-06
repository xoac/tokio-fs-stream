use super::error::Error;
use async_bincode::AsyncBincodeReader;
use serde::Deserialize;
use tokio_fs::File;

use futures::prelude::*;
use futures::try_ready;

use log::{debug, trace};

use std::io;
use std::path::{Path, PathBuf};

use futures::sync::mpsc;
use notify::{self, Watcher};
use std::sync::mpsc as std_mpsc;

type Rx<T> = mpsc::UnboundedReceiver<T>;

struct FileWatcher {
    rx: Rx<notify::RawEvent>,
    // we don't use them but prevent call drop.
    _thread: std::thread::JoinHandle<()>,
    _watcher: notify::RecommendedWatcher,
}

impl FileWatcher {
    pub fn watch_path<P: AsRef<Path>>(path: P) -> Result<FileWatcher, notify::Error> {
        let (tx, rx) = mpsc::unbounded();
        let (std_tx, std_rx) = std_mpsc::channel();
        let thread = std::thread::spawn(move || {
            while let Ok(item) = std_rx.recv() {
                if let Err(err) = tx.unbounded_send(item) {
                    // other side is droped.
                    trace!("Reciver will never recive {:?}", err);
                    break;
                }
            }
        });

        let mut watcher = notify::raw_watcher(std_tx)?;
        watcher.watch(path, notify::RecursiveMode::NonRecursive)?;
        Ok(FileWatcher {
            rx,
            _thread: thread,
            _watcher: watcher,
        })
    }
}

impl Stream for FileWatcher {
    type Item = notify::RawEvent;
    type Error = notify::Error;
    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        let item = self.rx.poll();
        match item {
            Ok(Async::Ready(Some(event))) => {
                if event.op.is_err() {
                    Err(event.op.unwrap_err())
                } else {
                    Ok(Async::Ready(Some(event)))
                }
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(_) => unreachable!(),
        }
    }
}

/// Stream thats read items from file with monitoring changes.
pub struct FileReciver<T> {
    reader: AsyncBincodeReader<File, T>,
    path: PathBuf,
    events_rx: Option<FileWatcher>,
}

pub fn new<T>(path: PathBuf) -> io::Result<FileReciver<T>> {
    let read_fd_std = std::fs::OpenOptions::new().read(true).open(path.clone())?;
    let read_file = File::from_std(read_fd_std);
    let reader: AsyncBincodeReader<File, T> = read_file.into();
    Ok(FileReciver {
        reader,
        path,
        events_rx: None,
    })
}

impl<T> FileReciver<T> {
    fn poll_watcher(&mut self) -> Poll<Option<()>, notify::Error> {
        debug_assert!(self.events_rx.is_some());

        let events_rx_mut = self.events_rx.as_mut().unwrap();
        let async_result = events_rx_mut.poll()?.map(|file_event| {
            debug!("Event from os {:?}", file_event);
            Some(())
        });

        Ok(async_result)
    }
}

impl<T> Stream for FileReciver<T>
where
    for<'a> T: Deserialize<'a>,
{
    type Item = T;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            trace!("poll reciver!");
            // tutaj jest zwracane None jesli jestesmy na koncu pliku.
            let opt_item = match self.reader.poll()? {
                Async::Ready(opt_item) => opt_item,
                Async::NotReady => return Ok(Async::NotReady),
            };

            if opt_item.is_some() {
                return Ok(Async::Ready(opt_item));
            } else {
                // check file is read_only:
                // - yes -- return None
                // - no  -- return NotReady - more data can be added.

                // getting inner instane with is tokio_fs::File
                // TODO replace using try_ready! when trace will be not necessery
                let async_item = self.reader.get_mut().poll_metadata()?;

                match async_item {
                    Async::Ready(metadata) => {
                        if metadata.permissions().readonly() {
                            trace!("File fully readed and marked readonly -- stream done!");
                            //TODO remove file -- keep state when polling!
                            //FIXME
                            try_ready!(tokio_fs::remove_file(&self.path).poll());
                            return Ok(Async::Ready(None));
                        } else {
                            trace!("Not ready - File not marked readonly!");
                            // TODO task::current().notify();
                            // create FileWatcher and read notifications.
                            if self.events_rx.is_none() {
                                let file_watcher =
                                    FileWatcher::watch_path(&self.path).expect("Working watcher");
                                self.events_rx = Some(file_watcher);
                                continue; // Sth could be added to file!
                            }
                        }
                    }
                    Async::NotReady => {
                        trace!("Not ready - poll metadata");
                    }
                }

                if self.events_rx.is_some() {
                    while let Some(_notify_file_was_changed) = try_ready!(self.poll_watcher()) {
                        trace!("iterate over event");
                    }
                }
            }
        }
    }
}

/// Stream to read serialized items `T` from dir.
pub struct DirReciver<T> {
    dir_path: PathBuf,
    file: FileReciver<T>,
    next_file_index: usize,
}

pub fn new_dir_reciver<T>(dir_path: PathBuf) -> io::Result<DirReciver<T>> {
    if !dir_path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Path {:?} dosen't represent dir", dir_path),
        ));
    }

    let next_file_index = 0; //TODO powinno byc sprawdzone czy nie ma juz plikow w katalogu.

    let mut file_path = dir_path.clone();
    file_path.push(next_file_index.to_string());

    Ok(DirReciver {
        dir_path,
        file: new(file_path)?,
        next_file_index: next_file_index + 1,
    })
}

impl<T> DirReciver<T> {
    fn use_next_file(&mut self) -> Result<Option<FileReciver<T>>, io::Error> {
        let mut path = self.dir_path.clone();
        path.push(self.next_file_index.to_string());
        self.next_file_index += 1;

        match new(path).map(Some) {
            Err(err) => match err.kind() {
                io::ErrorKind::NotFound => Ok(None),
                _ => Err(err),
            },
            forward => forward,
        }
    }
}

impl<T> Stream for DirReciver<T>
where
    for<'a> T: Deserialize<'a>,
{
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match try_ready!(self.file.poll()) {
                None => {
                    match self.use_next_file()? {
                        Some(file) => self.file = file,
                        None => return Ok(Async::Ready(None)),
                    };
                }
                some_item => return Ok(Async::Ready(some_item)),
            }
        }
    }
}
