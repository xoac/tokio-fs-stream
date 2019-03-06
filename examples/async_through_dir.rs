use futures::stream::iter_ok;
use pretty_env_logger;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::prelude::*;
use tokio::timer::Delay;
use tokio_fs_stream::channel;

pub fn main() {
    pretty_env_logger::init();
    // we create sender and reciver throught fs.
    let (sender, reciver) = channel::unordered_dir_fs(PathBuf::from("test_dir"), 100).unwrap();

    // create some iterator witch Item is Serialize and Deserialize.
    let items = iter_ok::<_, std::io::Error>(vec![1u32, 2, 3, 4]).and_then(|item| {
        Delay::new(Instant::now() + Duration::from_millis(30))
            .map_err(|_err| unimplemented!())
            .and_then(move |_| Ok(item))
    });

    // Send all items to file.
    let future_sending = sender.send_all(items).map(drop).map_err(|err| {
        eprintln!("Sending error! {:?}", err);
    });

    // Read all items from file. If program terminate or panic all items saved from into file will
    // be restored.
    let read_item = reciver
        .for_each(|item| {
            println!("Recived item {}", item);
            Ok(())
        })
        .map_err(|err| {
            eprintln!("reciver error {:?}", err);
        });

    tokio::run(futures::lazy(|| {
        tokio::spawn(read_item);
        tokio::spawn(future_sending);
        Ok(())
    }))
}
