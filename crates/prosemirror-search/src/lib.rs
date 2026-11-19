mod query;
mod search;

pub use query::{valid_regexp, QueryFilter, RegexMatch, Replacement, SearchQuery, SearchQueryConfig, SearchResult};
pub use search::{
    find_next, find_next_no_wrap, find_prev, find_prev_no_wrap, get_match_highlights, get_search_state,
    replace_all, replace_current, replace_next, replace_next_no_wrap, search,
    set_search_state, Dispatch, SearchOptions, SearchRange, SearchState, SEARCH_KEY,
};

pub const SEARCH_CSS: &str = include_str!("../style/search.css");
