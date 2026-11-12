use crate::RequestError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error(transparent)]
    PostArchiver(#[from] post_archiver::error::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
