pub mod content;
mod diff;
pub mod error;
pub mod mark;
pub mod node;
pub mod replace;
pub mod resolved_pos;
pub mod schema;
pub mod slice;

pub use error::ModelError;
pub use mark::Mark;
pub use node::{slice_utf16, Fragment, Node, NodeInner};
pub use replace::{replace, replace_at_pos};
pub use resolved_pos::{NodeRange, PathEntry, ResolvedPos};
pub use schema::{AttributeSpec, MarkSpec, MarkType, NodeSpec, NodeType, Schema};
pub use slice::Slice;
pub use content::ContentMatch;
pub use diff::DiffEnd;
