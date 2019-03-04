use bincode::Error as AsyncBinCodeError;
use custom_error::custom_error;
use notify::Error as NotifyError;

custom_error! { pub Error
    AsyncBinCode { source: AsyncBinCodeError } = "Async bin code error {source}",
    NotifyError { source: NotifyError } = "Notify error {source}",
}

impl From<std::io::Error> for Error {
    #[inline]
    fn from(oth: std::io::Error) -> Self {
        Error::AsyncBinCode {
            source: AsyncBinCodeError::from(oth),
        }
    }
}
