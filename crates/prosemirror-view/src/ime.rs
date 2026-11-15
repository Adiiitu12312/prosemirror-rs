use unicode_segmentation::UnicodeSegmentation;
use prosemirror_state::{EditorState, Selection, Transaction};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeState {
    Idle,
    Composing {
        from: usize,
        to: usize,
        text: String,
    },
}

pub struct ImeHandler {
    pub state: ImeState,
}

impl ImeHandler {
    pub fn new() -> Self {
        Self {
            state: ImeState::Idle,
        }
    }

    pub fn handle_composition_start(&mut self, editor_state: &EditorState) {
        let from = editor_state.selection.from();
        let to = editor_state.selection.to(&editor_state.doc);
        self.state = ImeState::Composing {
            from,
            to,
            text: String::new(),
        };
    }

    pub fn handle_composition_update(
        &mut self,
        editor_state: &EditorState,
        composition_text: &str,
    ) -> Transaction {
        let (from, to) = match self.state {
            ImeState::Composing { from, to, .. } => (from, to),
            ImeState::Idle => {
                let f = editor_state.selection.from();
                let t = editor_state.selection.to(&editor_state.doc);
                (f, t)
            }
        };

        let mut tr = editor_state.tr();
        tr.insert_text(composition_text, from, to);
        let new_to = from + composition_text.encode_utf16().count();
        tr.set_selection(Selection::cursor(new_to));

        self.state = ImeState::Composing {
            from,
            to: new_to,
            text: composition_text.to_string(),
        };

        tr
    }

    pub fn handle_composition_end(
        &mut self,
        editor_state: &EditorState,
        committed_text: &str,
    ) -> Transaction {
        let (from, to) = match self.state {
            ImeState::Composing { from, to, .. } => (from, to),
            ImeState::Idle => {
                let f = editor_state.selection.from();
                let t = editor_state.selection.to(&editor_state.doc);
                (f, t)
            }
        };

        let mut tr = editor_state.tr();
        tr.insert_text(committed_text, from, to);
        let final_pos = from + committed_text.encode_utf16().count();
        tr.set_selection(Selection::cursor(final_pos));

        self.state = ImeState::Idle;
        tr
    }

    /// Hindi (Devanagari) conjuncts and complex script grapheme cluster boundary logic.
    /// Ensures deleting a grapheme cluster removes the entire conjunct rather than splitting
    /// a half-character (virama) into invalid standalone glyphs.
    pub fn delete_previous_grapheme_cluster(
        &self,
        editor_state: &EditorState,
    ) -> Option<Transaction> {
        let pos = editor_state.selection.from();
        if pos == 0 {
            return None;
        }

        let rpos = editor_state.doc.resolve(pos).ok()?;
        let parent = rpos.parent();
        let text_before = parent.text_between(0, rpos.parent_offset);
        
        let clusters: Vec<&str> = text_before.graphemes(true).collect();
        if let Some(last_cluster) = clusters.last() {
            let cluster_size = last_cluster.encode_utf16().count();
            let from = pos.saturating_sub(cluster_size);
            let mut tr = editor_state.tr();
            tr.delete(from, pos);
            Some(tr)
        } else {
            None
        }
    }
}
