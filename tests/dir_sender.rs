use futures::stream::iter_ok;
use std::io;
use tokio::prelude::*;
use tokio_fs_stream::channel::unordered_dir_fs;

#[test]
fn dir_sender_naive() {
    let (s, r) = unordered_dir_fs("dir_sender_test".into(), 100).expect("Folder should exist");

    let data = vec![
        "Ala ma kota".to_string(),
        "Ala ma kota".to_string(),
        "Ala ma kota".to_string(),
        "Ala ma kota".to_string(),
        "Ala ma kota".to_string(),
    ];

    let stream = iter_ok::<_, io::Error>(data.clone());

    let send_to_fs = s.send_all(stream);

    tokio::run(futures::lazy(move || {
        tokio::spawn(send_to_fs.map(drop).map_err(|err| eprintln!("Send to file {:?}", err)));

        tokio::spawn(
            r.collect()
                .and_then(move |readed| {
                    println!("Getting next item");
                    assert_eq!(readed, data);
                    Ok(())
                })
                .map_err(|err| eprintln!("Get from file {:?}", err)),
        );

        Ok(())
    }));
}
