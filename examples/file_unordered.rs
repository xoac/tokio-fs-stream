use futures::stream::iter_ok;
use futures::StartSend;
use pretty_env_logger;
use reqwest::r#async::{Client, Response};
use std::time::Duration;
use tokio::prelude::*;

use futures_retry::{RetryPolicy, SinkRetryExt};
use tokio_fs_stream::SinkFsExt;

// This is struct that implement Sink for ouer test.
struct PostSender {
    post_url: String,
    client: Client,
    sending_item: Option<String>,
    sending_fut: Option<Box<Future<Item = Response, Error = reqwest::Error> + Send>>,
}

impl PostSender {
    pub fn new(post_url: String) -> Self {
        Self {
            post_url,
            client: Client::new(),
            sending_item: None,
            sending_fut: None,
        }
    }

    fn try_get_fut(
        &mut self,
    ) -> Option<&mut Box<Future<Item = Response, Error = reqwest::Error> + Send>> {
        if self.sending_fut.is_none() {
            if let Some(ref item) = self.sending_item {
                let fut = self.client.post(&self.post_url).body(item.clone()).send();
                self.sending_fut = Some(Box::new(fut));
            }
        }

        self.sending_fut.as_mut()
    }

    fn ready_get_next_item(&mut self) {
        self.sending_item.take();
        self.sending_fut.take();
    }
}

impl Sink for PostSender {
    type SinkItem = String;
    type SinkError = reqwest::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if self.sending_item.is_none() {
            self.sending_item = Some(item);
            self.poll_complete()?;
            Ok(AsyncSink::Ready)
        } else {
            // we can return NotReady,
            match self.poll_complete()? {
                Async::Ready(()) => self.start_send(item),
                Async::NotReady => Ok(AsyncSink::NotReady(item)),
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match self.try_get_fut() {
            Some(fut) => {
                let response = match fut.poll() {
                    Err(err) => {
                        self.sending_fut.take();
                        return Err(err);
                    }
                    Ok(Async::Ready(item)) => item,
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                };
                self.ready_get_next_item();
                let _ok = response.error_for_status()?;
                Ok(Async::Ready(()))
            }
            None => {
                self.sending_item.take();
                Ok(Async::Ready(()))
            }
        }
    }
}

use std::io;
pub fn main() -> io::Result<()> {
    pretty_env_logger::init();

    // items we want send to serwer.
    let stream = iter_ok::<_, reqwest::Error>(vec![
        "Ala ma kota".to_string(),
        "Kot ma ale".to_string(),
        "你好".to_string(),
    ])
    .and_then(|item| {
        println!("Consumed {:?}", item);
        Ok(item)
    });

    // sink sending item to server, sometimes resolving to error.
    // the grpc sink would be probably a better example of sink implementation, but more people
    // know http.
    let sink = PostSender::new("http://httpbin.org/status/200,408,500,500,408".to_string());

    // we wanna retry on some error. In case error is returned - item is saved on this as backup.
    use reqwest::StatusCode;
    let error_handler = |e: reqwest::Error| -> RetryPolicy<reqwest::Error> {
        if let Some(status) = e.status() {
            return match status {
                StatusCode::INTERNAL_SERVER_ERROR => RetryPolicy::WaitRetry(Duration::from_secs(5)),
                StatusCode::REQUEST_TIMEOUT => RetryPolicy::Repeat,
                _ => RetryPolicy::ForwardError(e),
            };
        }
        RetryPolicy::ForwardError(e)
    };

    let write_stream_inside_sink = sink
        .retry(error_handler) // repeat on some errors. When error occure and RetryPolicy::WaitRetry is used error is ignored and translated to Ok(AsyncSink::NotReady(item)).
        .send_all_fs_backpresure(stream, "dir_sender_test".into())?; // save items in `dir_sender_test` when AsyncSink::NotReady(item) is returned.

    tokio::run(
        write_stream_inside_sink
            .map(drop)
            .map_err(|err| eprintln!("{:?}", err)),
    );
    Ok(())
}
