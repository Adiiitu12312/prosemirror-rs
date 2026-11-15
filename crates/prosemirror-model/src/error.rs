use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    #[error("Position {0} is out of bounds (document size: {1})")]
    OutOfBounds(usize, usize),
    #[error("Unknown node type: {0}")]
    UnknownNodeType(String),
    #[error("Unknown mark type: {0}")]
    UnknownMarkType(String),
    #[error("Invalid content for node '{0}'")]
    InvalidContent(String),
    #[error("Invalid slice open depths: start {0}, end {1}")]
    InvalidSlice(usize, usize),
    #[error("Replace error: {0}")]
    ReplaceError(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}
