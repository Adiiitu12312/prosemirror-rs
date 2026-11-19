//! Rust port of prosemirror-markdown 1.13.4.

mod from_markdown;
mod schema;
mod to_markdown;
mod tokenizer;

pub use from_markdown::{default_markdown_parser, GetAttrs, MarkdownParser, ParseSpec};
pub use schema::schema;
pub use to_markdown::{
    default_markdown_serializer, MarkDelimiter, MarkSerializerSpec, MarkdownSerializer,
    MarkdownSerializerOptions, MarkdownSerializerState, NodeSerializer,
};
pub use tokenizer::{CommonMarkTokenizer, MarkdownTokenizer, Token};

/// The tokenizer, parser, or serializer could not handle the supplied input.
#[derive(Debug, thiserror::Error)]
pub enum MarkdownError {
    #[error(transparent)]
    Model(#[from] prosemirror_model::ModelError),
    #[error("Token type `{0}` not supported by Markdown parser")]
    UnsupportedToken(String),
    #[error("Token type `{0}` not supported by Markdown renderer")]
    UnsupportedNode(String),
    #[error("Mark type `{0}` not supported by Markdown renderer")]
    UnsupportedMark(String),
    #[error("Unrecognized parsing spec for token `{0}`")]
    InvalidSpec(String),
    #[error("Markdown token stream closed a node without an open node")]
    UnmatchedClose,
    #[error("The Markdown schema could not create its top node")]
    MissingDocument,
}
