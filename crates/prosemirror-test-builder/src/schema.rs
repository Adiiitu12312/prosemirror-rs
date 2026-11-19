use std::sync::OnceLock;

use prosemirror_model::Schema;
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_schema_list::add_list_nodes;

pub fn test_schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        Schema::new(add_list_nodes(nodes(), "paragraph block*", Some("block")), marks())
    })
}
