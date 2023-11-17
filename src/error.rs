use thiserror::Error;

use crate::id::Id;

#[derive(Debug, Error, Clone, Copy)]
#[error("Node not found {id}")]
pub struct NodeNotFoundError {
    pub id: Id,
}

#[derive(Debug, Error, Clone, Copy)]
pub enum RunEveError {
    #[error(transparent)]
    NodeNotFound(#[from] NodeNotFoundError),
}

#[derive(Debug, Error, Clone, Copy)]
#[error("Downcast error {id}")]
pub struct DowncastError {
    pub id: Id,
}

#[derive(Debug, Error, Clone, Copy)]
pub enum ReplyChannelError {
    #[error("Oneshot channel closed")]
    OneshotClosed,
}

#[derive(Debug, Error, Clone, Copy)]
#[error("Reactive channel closed")]
pub struct ReactivelyChannelError {}

#[derive(Debug, Error, Clone, Copy)]
pub enum GetNodeValueError {
    #[error(transparent)]
    NodeNotFound(#[from] NodeNotFoundError),
    #[error(transparent)]
    Downcast(#[from] DowncastError),
    #[error(transparent)]
    ReplyChannel(#[from] ReplyChannelError),
    #[error(transparent)]
    ReactivelyChannel(#[from] ReactivelyChannelError),
}

#[derive(Debug, Error, Clone, Copy)]
pub enum SetNodeValueError {
    #[error(transparent)]
    NodeNotFound(#[from] NodeNotFoundError),
    #[error(transparent)]
    Downcast(#[from] DowncastError),
    #[error(transparent)]
    ReplyChannel(#[from] ReplyChannelError),
    #[error(transparent)]
    ReactivelyChannel(#[from] ReactivelyChannelError),
}

impl From<GetNodeValueError> for SetNodeValueError {
    fn from(e: GetNodeValueError) -> Self {
        match e {
            GetNodeValueError::NodeNotFound(e) => Self::NodeNotFound(e),
            GetNodeValueError::Downcast(e) => Self::Downcast(e),
            GetNodeValueError::ReplyChannel(e) => Self::ReplyChannel(e),
            GetNodeValueError::ReactivelyChannel(e) => Self::ReactivelyChannel(e),
        }
    }
}
