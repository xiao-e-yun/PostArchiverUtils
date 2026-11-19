use crate::RequestError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error(transparent)]
    PostArchiver(#[from] post_archiver::error::Error),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::PostArchiver(post_archiver::error::Error::Io(err))
    }
}

impl From<post_archiver::utils::rusqlite::Error> for Error {
    fn from(err: post_archiver::utils::rusqlite::Error) -> Self {
        Error::PostArchiver(post_archiver::error::Error::Rusqlite(err))
    }
}
