mod change;
mod changeset;
mod diff;
mod simplify;

pub use change::{Change, Span};
pub use changeset::{ChangeSet, ChangedRange};
pub use diff::{compute_diff as compute_diff_with_encoder, DefaultEncoder, DefaultToken, TokenEncoder};
pub use simplify::simplify_changes;
