use prosemirror_model::Node;
use prosemirror_state::EditorState;

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub struct GlyphCluster {
    pub pos: usize,
    pub len: usize,
    pub rect: LayoutRect,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct LineLayout {
    pub rect: LayoutRect,
    pub clusters: Vec<GlyphCluster>,
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Debug)]
pub struct ViewLayout {
    pub lines: Vec<LineLayout>,
    pub width: f32,
    pub height: f32,
}

impl ViewLayout {
    pub fn build(state: &EditorState, max_width: f32) -> Self {
        let mut lines = Vec::new();
        let mut cur_y = 0.0;
        let line_height = 24.0;
        let char_width = 8.5; // Default glyph metric approximation for shaping backend

        let mut doc_pos = 0;
        let doc = &state.doc;

        for block in &doc.content().children {
            doc_pos += 1; // Open tag
            let block_start = doc_pos;
            let mut cur_x = 0.0;
            let mut clusters = Vec::new();

            for inline_node in &block.content().children {
                if inline_node.is_text() {
                    let text = inline_node.text().unwrap_or("");
                    let mut cluster_pos = doc_pos;
                    for ch in text.chars() {
                        let w = char_width;
                        let u16_len = ch.len_utf16();
                        clusters.push(GlyphCluster {
                            pos: cluster_pos,
                            len: u16_len,
                            rect: LayoutRect {
                                x: cur_x,
                                y: cur_y,
                                width: w,
                                height: line_height,
                            },
                            text: ch.to_string(),
                        });
                        cur_x += w;
                        cluster_pos += u16_len;
                    }
                    doc_pos += inline_node.node_size();
                }
            }

            let block_end = doc_pos;
            lines.push(LineLayout {
                rect: LayoutRect {
                    x: 0.0,
                    y: cur_y,
                    width: cur_x.min(max_width),
                    height: line_height,
                },
                clusters,
                from: block_start,
                to: block_end,
            });

            cur_y += line_height + 4.0;
            doc_pos += 1; // Close tag
        }

        Self {
            lines,
            width: max_width,
            height: cur_y,
        }
    }

    pub fn pos_to_coords(&self, pos: usize) -> LayoutRect {
        for line in &self.lines {
            for cluster in &line.clusters {
                if cluster.pos == pos {
                    return LayoutRect {
                        x: cluster.rect.x,
                        y: cluster.rect.y,
                        width: 2.0,
                        height: cluster.rect.height,
                    };
                }
            }
            if pos == line.to {
                return LayoutRect {
                    x: line.rect.x + line.rect.width,
                    y: line.rect.y,
                    width: 2.0,
                    height: line.rect.height,
                };
            }
        }
        LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 24.0,
        }
    }

    pub fn coords_to_pos(&self, x: f32, y: f32) -> usize {
        for line in &self.lines {
            if y >= line.rect.y && y <= line.rect.y + line.rect.height {
                for cluster in &line.clusters {
                    if x <= cluster.rect.x + (cluster.rect.width / 2.0) {
                        return cluster.pos;
                    }
                }
                return line.to;
            }
        }
        self.lines.last().map(|l| l.to).unwrap_or(0)
    }
}
