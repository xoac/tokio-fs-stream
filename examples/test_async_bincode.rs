extern crate async_bincode;
extern crate futures;
extern crate tokio;
extern crate tokio_fs;

use futures::future;
use tokio::prelude::*;

use async_bincode::*;
use std::time::{Duration, Instant};
use tokio_fs::File;

pub fn main() {
    let writer = File::create("test");
    let reader = File::open("test");

    let lazy_worker = future::lazy(|| {
        let writer = writer.wait().unwrap();
        let reader = reader.wait().unwrap();

        let async_reader: AsyncBincodeReader<File, u32> = reader.into();
        let async_writer: AsyncBincodeWriter<File, u32, SyncDestination> = writer.into();
        let async_writer = async_writer.for_async();

        let mut item = 5u32;
        let stream_of_items = tokio::timer::Interval::new(Instant::now(), Duration::from_secs(1))
            .and_then(move |_| {
                item += 1;
                println!("Sending {:?}", item);
                Ok(item)
            }).map_err(|err| panic!("Error {:?}", err));

        let sending_worker = async_writer
            .sink_map_err(|err| panic!("Sink map err {:?}", err))
            .send_all(stream_of_items)
            .map_err(|_| {})
            .map(|_items| println!("Sending end!"));
        tokio::spawn(sending_worker);

        let reading_stream = tokio::timer::Delay::new(Instant::now() + Duration::from_secs(5))
            .map_err(|_| {})
            .and_then(|_| {
                async_reader
                    .for_each(|item| {
                        println!("Received item {}", item);
                        Ok(())
                    }).map_err(|err| panic!("Reading stream error {:?}", err))
                    .map(|_| println!("Reading eneded"))
            });
        tokio::spawn(reading_stream);
        Ok(())
    });

    tokio::run(lazy_worker);
}
