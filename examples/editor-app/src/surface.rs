//! CPU rendering and document geometry for the native editor example.

use std::cell::{Cell, OnceCell, RefCell};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::Path;

use cosmic_text::{Attrs, BidiParagraphs, Buffer, Color, Family, FontSystem, Metrics, PhysicalGlyph, Shaping, Style, SwashCache, Weight};
use prosemirror_editor::{
    BlockUpdate, ClipboardContent, CompositionDisplay, CursorIcon, DisplayItem, DisplayList, DisplayNode,
    Host, Layout, LineBox, Point, PositionAtCoords, Rect, SelectionKind, accessibility_tree,
};
use prosemirror_model::{table_map::TableMap, Mark, Node};
use prosemirror_state::EditorState;
use prosemirror_view::{AccessibilityTreeUpdate, Decoration, DecorationType};
use unicode_segmentation::UnicodeSegmentation;

use crate::block_index::{BlockIndex, Extent};

const PAGE_INSET: f64 = 24.0;
const BLOCK_GAP: f64 = 10.0;
const EAGER_BLOCKS: usize = 128;
const PAPER: Color = Color::rgb(251, 252, 254);
const INK: Color = Color::rgb(29, 39, 54);
const MUTED: Color = Color::rgb(92, 107, 125);
const BLUE: Color = Color::rgb(29, 101, 188);
const SELECTION: Color = Color::rgba(80, 151, 234, 82);
const GRID: Color = Color::rgb(181, 194, 210);
const CODE: Color = Color::rgb(241, 244, 248);
const SANS_FAMILY: &str = "Inter";
const MONO_FAMILY: &str = "JetBrains Mono";
const BUNDLED_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/Inter-Regular.ttf"),
    include_bytes!("../assets/Inter-Bold.otf"),
    include_bytes!("../assets/Inter-Italic.otf"),
    include_bytes!("../assets/Inter-BoldItalic.otf"),
    include_bytes!("../assets/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/JetBrainsMono-BoldItalic.ttf"),
];

pub struct SurfaceHost {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    fonts: RefCell<FontSystem>,
    raster: SwashCache,
    cached_width: u32,
    blocks: Vec<PlacedBlock>,
    index: RefCell<BlockIndex>,
    scroll: Point,
    content_width: Cell<f64>,
    document_size: usize,
    clipboard: Option<ClipboardContent>,
    candidate: Option<Rect>,
    cursor: CursorIcon,
    focus_requested: bool,
    accessibility_state: Option<EditorState>,
    accessibility: OnceCell<AccessibilityTreeUpdate>,
    drop_line: Option<Rect>,
    drag_ghost: Option<(Point, String)>,
    touch_handles: Vec<Rect>,
}

impl SurfaceHost {
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_fonts(width, height, bundled_fonts(true))
    }

    /// Use the bundled SIL-licensed Inter font for reproducible headless output.
    pub fn deterministic(width: u32, height: u32) -> Self {
        Self::with_fonts(width, height, bundled_fonts(false))
    }

    /// Construct a surface using caller-provided fonts, with no system font dependency.
    pub fn with_font_data(width: u32, height: u32, font_data: Vec<Vec<u8>>) -> Self {
        let mut database = fontdb::Database::new();
        for data in font_data {
            database.load_font_data(data);
        }
        let families = database.faces().filter_map(|face| face.families.first()
            .map(|(name, _)| (name.clone(), face.monospaced))).collect::<Vec<_>>();
        if let Some((first, _)) = families.iter().find(|(_, mono)| !mono).or_else(|| families.first()) {
            database.set_sans_serif_family(first);
        }
        if let Some((first, _)) = families.iter().find(|(_, mono)| *mono).or_else(|| families.first()) {
            database.set_monospace_family(first);
        }
        Self::with_fonts(width, height, FontSystem::new_with_locale_and_db("en-US".into(), database))
    }

    fn with_fonts(width: u32, height: u32, fonts: FontSystem) -> Self {
        let mut surface = Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            fonts: RefCell::new(fonts),
            raster: SwashCache::new(),
            cached_width: 0,
            blocks: Vec::new(),
            index: RefCell::new(BlockIndex::default()),
            scroll: Point::default(),
            content_width: Cell::new(0.0),
            document_size: 0,
            clipboard: None,
            candidate: None,
            cursor: CursorIcon::Text,
            focus_requested: false,
            accessibility_state: None,
            accessibility: OnceCell::new(),
            drop_line: None,
            drag_ghost: None,
            touch_handles: Vec::new(),
        };
        surface.resize(width, height);
        surface
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.pixels.resize(self.width as usize * self.height as usize, PAPER.0 & 0x00ff_ffff);
        self.limit_scroll();
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The software drawable in softbuffer's 0x00RRGGBB format.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        let mut target = BufWriter::new(File::create(path)?);
        self.write_png(&mut target)?;
        target.flush()?;
        Ok(())
    }

    pub fn write_png(&self, target: impl Write) -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(target, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let bytes = self.pixels.iter().flat_map(|pixel| {
            [(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]
        }).collect::<Vec<_>>();
        writer.write_image_data(&bytes)?;
        writer.finish()
    }

    /// Convert a pointer's window position to the coordinate system used by Layout.
    pub fn document_point(&self, window: Point) -> Point {
        Point { x: window.x + self.scroll.x, y: window.y + self.scroll.y }
    }

    pub fn cursor_icon(&self) -> CursorIcon {
        self.cursor
    }

    pub fn take_focus_request(&mut self) -> bool {
        std::mem::take(&mut self.focus_requested)
    }

    /// The native IME rectangle in window coordinates.
    pub fn ime_candidate_rect(&self) -> Option<Rect> {
        self.candidate.map(|rect| translate(rect, -self.scroll.x, -self.scroll.y))
    }

    pub fn accessibility(&self) -> Option<&AccessibilityTreeUpdate> {
        self.accessibility.get().or_else(|| self.accessibility_state.as_ref()
            .map(|state| self.accessibility.get_or_init(|| accessibility_tree(state))))
    }

    /// Position transient drag graphics in document coordinates.
    pub fn set_drag_feedback(&mut self, drop_line: Option<Rect>, ghost: Option<(Point, String)>) {
        self.drop_line = drop_line;
        self.drag_ghost = ghost;
    }

    /// Set touch handle rectangles in document coordinates.
    pub fn set_touch_handles(&mut self, handles: Vec<Rect>) {
        self.touch_handles = handles;
    }

    fn limit_scroll(&mut self) {
        self.scroll.x = self.scroll.x.clamp(0.0, (self.content_width.get() - f64::from(self.width)).max(0.0));
        let total_height = self.index.borrow().before(self.blocks.len()).height + PAGE_INSET * 2.0;
        self.scroll.y = self.scroll.y.clamp(0.0, (total_height - f64::from(self.height)).max(0.0));
    }

    fn prepare_layout(&mut self, frame: &DisplayList, changed: &BlockUpdate) {
        let regenerate = self.cached_width != self.width;
        self.cached_width = self.width;
        self.document_size = frame.document_size;
        if let Some(range) = &frame.block_range {
            for (index, source) in range.clone().zip(&frame.blocks) {
                let old = &self.blocks[index];
                let extent = old.extent();
                let block = PlacedBlock::new(source, frame.composition.as_ref(), extent.height - BLOCK_GAP);
                self.index.get_mut().replace(index, extent, block.extent());
                self.blocks[index] = block;
            }
        } else {
            let mut previous = std::mem::take(&mut self.blocks).into_iter().map(Some).collect::<Vec<_>>();
            self.blocks.reserve(frame.blocks.len());
            for (index, block) in frame.blocks.iter().enumerate() {
                let previous_index = if index < changed.current.start {
                    Some(index)
                } else if index >= changed.current.end {
                    Some(changed.previous.end + (index - changed.current.end))
                } else {
                    None
                };
                let prior = previous_index.and_then(|i| previous.get_mut(i)).and_then(Option::take);
                let mut next = PlacedBlock::new(block, frame.composition.as_ref(), 24.0);
                if let Some(prior) = prior.filter(|prior| !regenerate && prior.composition == next.composition
                    && same_display_node(&prior.source, block, prior.source.from, block.from))
                {
                    next.measured = prior.measured;
                    next.estimate = prior.estimate;
                }
                self.blocks.push(next);
            }
            self.index.get_mut().reset(self.blocks.iter().map(PlacedBlock::extent));
            self.content_width.set(self.blocks.iter().filter_map(|block| block.measured.get().map(|m| m.width))
                .fold(f64::from(self.width), f64::max));
        }
        if self.blocks.len() <= EAGER_BLOCKS {
            for index in 0..self.blocks.len() {
                self.measure_block(index);
            }
        }
        self.limit_scroll();
        self.measure_viewport();
    }

    fn measure_block(&self, index: usize) -> &MeasuredBlock {
        let block = &self.blocks[index];
        block.measured.get_or_init(|| {
            let mut fonts = self.fonts.borrow_mut();
            let mut shaper = Shaper { fonts: &mut fonts, base: block.source.from, composition: block.composition.as_ref() };
            let mut scene = Scene::default();
            let available = (f64::from(self.width) - PAGE_INSET * 2.0).max(20.0);
            let height = shaper.layout_node(&block.source, PAGE_INSET, 0.0, available, &mut scene);
            let width = scene.nodes.iter().map(|node| node.rect.right + PAGE_INSET).fold(0.0, f64::max);
            self.content_width.set(self.content_width.get().max(width));
            self.index.borrow_mut().replace(index, block.extent(), Extent {
                units: block.source.to - block.source.from, height: height + BLOCK_GAP,
            });
            MeasuredBlock { scene, height, width }
        })
    }

    fn measure_viewport(&self) -> Range<usize> {
        let start = self.block_at_y(self.scroll.y);
        let bottom = self.scroll.y + f64::from(self.height);
        let mut end = start;
        while end < self.blocks.len() && self.origin(end) < bottom {
            self.measure_block(end);
            end += 1;
        }
        start..end
    }

    fn block_at_y(&self, y: f64) -> usize {
        self.index.borrow().find(|sum| PAGE_INSET + sum.height <= y)
    }

    fn origin(&self, index: usize) -> f64 {
        PAGE_INSET + self.index.borrow().before(index).height
    }

    fn position(&self, index: usize) -> usize {
        self.index.borrow().before(index).units
    }

    fn block_at_pos(&self, pos: usize) -> usize {
        self.index.borrow().find(|sum| sum.units < pos)
    }
}

fn bundled_fonts(system_fallback: bool) -> FontSystem {
    let mut database = fontdb::Database::new();
    for bytes in BUNDLED_FONTS {
        database.load_font_data(bytes.to_vec());
    }
    if system_fallback {
        database.load_system_fonts();
    }
    database.set_sans_serif_family(SANS_FAMILY);
    database.set_monospace_family(MONO_FAMILY);
    FontSystem::new_with_locale_and_db("en-US".into(), database)
}

struct PlacedBlock {
    source: DisplayNode,
    composition: Option<CompositionDisplay>,
    estimate: f64,
    measured: OnceCell<MeasuredBlock>,
}

impl PlacedBlock {
    fn new(source: &DisplayNode, composition: Option<&CompositionDisplay>, estimate: f64) -> Self {
        let composition = composition.filter(|item| {
            source.content_from <= item.from && item.to <= source.content_to
        }).map(|item| CompositionDisplay {
            from: item.from - source.from,
            to: item.to - source.from,
            text: item.text.clone(),
            selection: item.selection.clone(),
        });
        Self { source: source.clone(), composition, estimate, measured: OnceCell::new() }
    }

    fn extent(&self) -> Extent {
        let height = self.measured.get().map_or(self.estimate, |block| block.height);
        Extent { units: self.source.to - self.source.from, height: height + BLOCK_GAP }
    }
}

struct MeasuredBlock {
    height: f64,
    width: f64,
    scene: Scene,
}

fn same_display_node(old: &DisplayNode, new: &DisplayNode, old_base: usize, new_base: usize) -> bool {
    old.node.ptr_eq(&new.node)
        && same_decorations(&old.decorations, &new.decorations, old_base, new_base)
        && old.children.len() == new.children.len()
        && old.children.iter().zip(&new.children).all(|(old, new)| match (old, new) {
            (DisplayItem::Node(old), DisplayItem::Node(new)) => same_display_node(old, new, old_base, new_base),
            (DisplayItem::Text(old), DisplayItem::Text(new)) => {
                old.from - old_base == new.from - new_base && old.to - old_base == new.to - new_base
                    && old.marks == new.marks && same_decorations(&old.decorations, &new.decorations, old_base, new_base)
            }
            (DisplayItem::Widget(old), DisplayItem::Widget(new)) => {
                old.pos - old_base == new.pos - new_base && old.marks == new.marks
                    && same_decoration(&old.decoration, &new.decoration, old_base, new_base)
            }
            _ => false,
        })
}

fn same_decorations(old: &[Decoration], new: &[Decoration], old_base: usize, new_base: usize) -> bool {
    old.len() == new.len() && old.iter().zip(new).all(|(old, new)| same_decoration(old, new, old_base, new_base))
}

fn same_decoration(old: &Decoration, new: &Decoration, old_base: usize, new_base: usize) -> bool {
    old.from >= old_base && new.from >= new_base
        && old.from - old_base == new.from - new_base && old.to - old_base == new.to - new_base
        && old.kind == new.kind && old.spec() == new.spec()
}

#[derive(Default)]
struct Scene {
    glyphs: Vec<Glyph>,
    lines: Vec<VisualLine>,
    nodes: Vec<NodeGeometry>,
    fills: Vec<Fill>,
}

impl Scene {
    fn append(&mut self, mut other: Self, dy: f64) {
        for glyph in &mut other.glyphs {
            glyph.physical.y += dy.round() as i32;
        }
        for line in &mut other.lines {
            line.rect = translate(line.rect, 0.0, dy);
            for cluster in &mut line.clusters {
                cluster.rect = translate(cluster.rect, 0.0, dy);
            }
        }
        for node in &mut other.nodes {
            node.rect = translate(node.rect, 0.0, dy);
        }
        for fill in &mut other.fills {
            fill.rect = translate(fill.rect, 0.0, dy);
        }
        self.glyphs.extend(other.glyphs);
        self.lines.extend(other.lines);
        self.nodes.extend(other.nodes);
        self.fills.extend(other.fills);
    }

    fn fill(&mut self, rect: Rect, color: Color) {
        self.fills.push(Fill { rect, color });
    }

    fn border(&mut self, rect: Rect, color: Color) {
        self.fill(Rect { bottom: rect.top + 1.0, ..rect }, color);
        self.fill(Rect { top: rect.bottom - 1.0, ..rect }, color);
        self.fill(Rect { right: rect.left + 1.0, ..rect }, color);
        self.fill(Rect { left: rect.right - 1.0, ..rect }, color);
    }
}

struct Glyph {
    physical: PhysicalGlyph,
    color: Color,
}

struct Fill {
    rect: Rect,
    color: Color,
}

struct NodeGeometry {
    from: usize,
    to: usize,
    rect: Rect,
}

struct VisualLine {
    from: usize,
    to: usize,
    inside: usize,
    rect: Rect,
    clusters: Vec<Cluster>,
}

struct Cluster {
    model: Range<usize>,
    preedit: Option<Range<usize>>,
    rect: Rect,
    rtl: bool,
}

impl Cluster {
    fn edge(&self, start: bool) -> Rect {
        let x = if start ^ self.rtl { self.rect.left } else { self.rect.right };
        Rect { left: x, right: x, ..self.rect }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextStyle {
    color: Color,
    background: Option<Color>,
    bold: bool,
    italic: bool,
    mono: bool,
    underline: bool,
    strikethrough: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { color: INK, background: None, bold: false, italic: false, mono: false, underline: false, strikethrough: false }
    }
}

impl TextStyle {
    fn for_block(node: &DisplayNode) -> (Self, Metrics) {
        let kind = node.node.type_name();
        let heading = node.node.attrs().and_then(|attrs| attrs["level"].as_u64()).filter(|_| kind == "heading");
        let metrics = match heading {
            Some(1) => Metrics::new(31.0, 39.0),
            Some(2) => Metrics::new(26.0, 34.0),
            Some(3) => Metrics::new(22.0, 30.0),
            Some(_) => Metrics::new(19.0, 26.0),
            None => Metrics::new(16.0, 24.0),
        };
        let style = Self { bold: heading.is_some(), mono: kind == "code_block", ..Self::default() }
            .with_decoration(&node.decorations);
        (style, metrics)
    }

    fn attrs(self) -> Attrs<'static> {
        Attrs::new()
            .family(if self.mono { Family::Monospace } else { Family::SansSerif })
            .weight(if self.bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if self.italic { Style::Italic } else { Style::Normal })
            .color(self.color)
    }

    fn with_decoration(mut self, decorations: &[Decoration]) -> Self {
        for decoration in decorations {
            let (DecorationType::Inline { attrs, .. } | DecorationType::Node { attrs }) = &decoration.kind else { continue };
            if attrs.get("class").is_some_and(|name| name.contains("search-match")) {
                self.background = Some(Color::rgb(255, 229, 151));
            }
            if let Some(background) = attrs.get("background").and_then(|value| parse_color(value)) {
                self.background = Some(background);
            }
            if let Some(foreground) = attrs.get("foreground").and_then(|value| parse_color(value)) {
                self.color = foreground;
            }
            self.bold |= attrs.get("bold").is_some_and(|value| value == "true");
            self.italic |= attrs.get("italic").is_some_and(|value| value == "true");
            self.underline |= attrs.get("underline").is_some_and(|value| value == "true");
        }
        self
    }

    fn with_marks(mut self, marks: &[Mark]) -> Self {
        for mark in marks {
            match mark.type_name.as_str() {
                "strong" | "bold" => self.bold = true,
                "em" | "italic" => self.italic = true,
                "code" => {
                    self.mono = true;
                    self.background = Some(CODE);
                }
                "link" => {
                    self.color = BLUE;
                    self.underline = true;
                }
                "underline" => self.underline = true,
                "strike" | "strikethrough" => self.strikethrough = true,
                _ => {}
            }
            if let Some(color) = mark.attrs.as_ref().and_then(|attrs| attrs["color"].as_str()).and_then(parse_color) {
                self.color = color;
            }
        }
        self
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let color = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            3 => Some(Color::rgb(((color >> 8) as u8 & 15) * 17, ((color >> 4) as u8 & 15) * 17, (color as u8 & 15) * 17)),
            6 => Some(Color::rgb((color >> 16) as u8, (color >> 8) as u8, color as u8)),
            8 => Some(Color::rgba((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8, color as u8)),
            _ => None,
        };
    }
    match value {
        "black" => Some(Color::rgb(0, 0, 0)),
        "white" => Some(Color::rgb(255, 255, 255)),
        "red" => Some(Color::rgb(186, 31, 44)),
        "blue" => Some(BLUE),
        "yellow" => Some(Color::rgb(255, 235, 137)),
        _ => None,
    }
}

#[derive(Clone)]
struct Unit {
    value: u16,
    model: Range<usize>,
    preedit: Option<usize>,
    style: TextStyle,
    object: bool,
}

struct TextSpan {
    bytes: Range<usize>,
    model: Range<usize>,
    preedit: Option<Range<usize>>,
    style: TextStyle,
    object: bool,
}

fn text_spans(units: &[Unit]) -> (String, Vec<TextSpan>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut index = 0;
    for character in char::decode_utf16(units.iter().map(|unit| unit.value)) {
        let ch = character.unwrap_or(char::REPLACEMENT_CHARACTER);
        let width = ch.len_utf16();
        let first = &units[index];
        let last = &units[index + width - 1];
        let start = text.len();
        text.push(ch);
        spans.push(TextSpan {
            bytes: start..text.len(),
            model: first.model.start..last.model.end,
            preedit: first.preedit.map(|offset| offset..offset + width),
            style: first.style,
            object: first.object,
        });
        index += width;
    }
    (text, spans)
}

struct Shaper<'a> {
    fonts: &'a mut FontSystem,
    base: usize,
    composition: Option<&'a CompositionDisplay>,
}

impl Shaper<'_> {
    fn layout_node(&mut self, node: &DisplayNode, x: f64, y: f64, width: f64, scene: &mut Scene) -> f64 {
        let kind = node.node.type_name();
        let role = node.node.node_type().and_then(|kind| kind.spec.table_role.as_deref());
        let mut measured_width = width;
        let height = if role == Some("table") {
            let (table_width, height) = self.layout_table(node, x, y, width, scene);
            measured_width = table_width;
            height
        } else if node.node.is_textblock() || node.node.is_text() || node.node.inline_content() {
            self.layout_text(node, x, y, width, scene)
        } else if node.node.is_leaf() {
            if kind == "horizontal_rule" {
                scene.fill(rect(x, y + 7.0, width, 1.0), GRID);
                15.0
            } else {
                let bounds = rect(x, y, width.min(240.0), 44.0);
                scene.fill(bounds, CODE);
                scene.border(bounds, GRID);
                self.label(kind, x + 8.0, y + 7.0, scene);
                44.0
            }
        } else {
            match kind {
                "blockquote" => {
                    let height = self.layout_children(node, x + 14.0, y + 2.0, (width - 14.0).max(12.0), scene) + 4.0;
                    scene.fill(rect(x, y, 3.0, height), GRID);
                    height
                }
                "ordered_list" | "bullet_list" | "task_list" => self.layout_list(node, x, y, width, scene),
                _ => self.layout_children(node, x, y, width, scene),
            }
        };
        scene.nodes.push(NodeGeometry {
            from: node.from - self.base,
            to: node.to - self.base,
            rect: rect(x, y, measured_width, height),
        });
        height
    }

    fn layout_children(&mut self, parent: &DisplayNode, x: f64, y: f64, width: f64, scene: &mut Scene) -> f64 {
        let mut height = 0.0;
        for child in parent.children.iter().filter_map(child_node) {
            height += self.layout_node(child, x, y + height, width, scene) + 5.0;
        }
        (height - 5.0).max(20.0)
    }

    fn layout_list(&mut self, node: &DisplayNode, x: f64, y: f64, width: f64, scene: &mut Scene) -> f64 {
        let kind = node.node.type_name();
        let start = node.node.attrs().and_then(|attrs| attrs["order"].as_u64()).unwrap_or(1);
        let mut height = 0.0;
        for (index, child) in node.children.iter().filter_map(child_node).enumerate() {
            let marker_y = y + height;
            match kind {
                "ordered_list" => self.label(&format!("{}.", start + index as u64), x + 2.0, marker_y + 1.0, scene),
                "task_list" => {
                    let checked = child.node.attrs().and_then(|attrs| attrs["checked"].as_bool()).unwrap_or(false);
                    task_checkbox(scene, x + 2.0, marker_y + 5.0, checked);
                }
                _ => self.label("•", x + 2.0, marker_y + 1.0, scene),
            }
            height += self.layout_node(child, x + 28.0, y + height, (width - 28.0).max(12.0), scene) + 5.0;
        }
        height.max(20.0)
    }

    fn layout_table(&mut self, node: &DisplayNode, x: f64, y: f64, width: f64, scene: &mut Scene) -> (f64, f64) {
        let Ok(map) = TableMap::get(&node.node) else { return (width, self.layout_children(node, x, y, width, scene)) };
        if map.width == 0 || map.height == 0 {
            return (width, self.layout_children(node, x, y, width, scene));
        }

        let rows = node.children.iter().filter_map(child_node).collect::<Vec<_>>();
        let mut cells = Vec::new();
        let mut widths: Vec<Option<f64>> = vec![None; map.width];
        for cell in rows.iter().flat_map(|row| row.children.iter().filter_map(child_node)) {
            let Ok(bounds) = map.find_cell(cell.from - node.content_from) else { continue };
            if let Some(explicit) = cell.node.attrs().and_then(|attrs| attrs["colwidth"].as_array()) {
                for (column, value) in widths[bounds.left..bounds.right].iter_mut().zip(explicit) {
                    if let Some(value) = value.as_f64().filter(|width| width.is_finite() && *width > 0.0) {
                        *column = Some(column.map_or(value, |previous| previous.max(value)));
                    }
                }
            }
            cells.push((cell, bounds));
        }
        let fixed: f64 = widths.iter().flatten().sum();
        let unset = widths.iter().filter(|value| value.is_none()).count();
        let normal = ((width - fixed) / unset.max(1) as f64).max(60.0);
        let mut x_offsets = vec![x];
        for width in widths {
            x_offsets.push(x_offsets.last().copied().unwrap_or(x) + width.unwrap_or(normal));
        }
        let mut heights = vec![34.0; map.height];
        let mut layouts = Vec::with_capacity(cells.len());
        for (cell, bounds) in cells {
            let left = x_offsets[bounds.left];
            let right = x_offsets[bounds.right];
            let mut content = Scene::default();
            let height = self.layout_children(cell, left + 7.0, 5.0, (right - left - 14.0).max(10.0), &mut content) + 10.0;
            let visible: f64 = heights[bounds.top..bounds.bottom].iter().sum();
            if height > visible {
                heights[bounds.bottom - 1] += height - visible;
            }
            layouts.push((cell, bounds, content));
        }

        let mut y_offsets = vec![y];
        for height in heights {
            y_offsets.push(y_offsets.last().copied().unwrap_or(y) + height);
        }
        for (index, row) in rows.iter().enumerate().take(map.height) {
            scene.nodes.push(NodeGeometry {
                from: row.from - self.base,
                to: row.to - self.base,
                rect: Rect { left: x, right: x_offsets[map.width], top: y_offsets[index], bottom: y_offsets[index + 1] },
            });
        }
        for (cell, bounds, content) in layouts {
            let cell_bounds = Rect {
                left: x_offsets[bounds.left],
                right: x_offsets[bounds.right],
                top: y_offsets[bounds.top],
                bottom: y_offsets[bounds.bottom],
            };
            let header = cell.node.node_type().and_then(|kind| kind.spec.table_role.as_deref()) == Some("header_cell");
            scene.fill(cell_bounds, if header { Color::rgb(232, 240, 249) } else { Color::rgb(255, 255, 255) });
            scene.border(cell_bounds, GRID);
            scene.nodes.push(NodeGeometry { from: cell.from - self.base, to: cell.to - self.base, rect: cell_bounds });
            scene.append(content, cell_bounds.top);
        }
        (x_offsets[map.width] - x, y_offsets[map.height] - y)
    }

    fn layout_text(&mut self, node: &DisplayNode, x: f64, y: f64, width: f64, scene: &mut Scene) -> f64 {
        let code_block = node.node.type_name() == "code_block";
        let (style, metrics) = TextStyle::for_block(node);
        let line_height = metrics.line_height;
        let from = node.content_from - self.base;
        let to = node.content_to - self.base;
        let mut units = Vec::new();
        collect_units(&node.children, self.base, style, &mut units);
        if let Some(composition) = self.composition.filter(|item| from <= item.from && item.to <= to) {
            let start = units.partition_point(|unit| unit.model.end <= composition.from);
            let end = units.partition_point(|unit| unit.model.start < composition.to).max(start);
            let mut style = units.get(start).map_or(style, |unit| unit.style);
            style.underline = true;
            style.color = BLUE;
            units.splice(start..end, composition.text.encode_utf16().enumerate().map(|(i, value)| Unit {
                value, model: composition.from..composition.to, preedit: Some(i), style, object: false,
            }));
        }
        let (text, spans) = text_spans(&units);
        let padding = if code_block { 8.0 } else { 0.0 };
        let (inner_x, inner_y) = (x + padding, y + padding);
        let inner_width = (width - 2.0 * padding).max(10.0);
        let mut text_scene = Scene::default();
        let buffer = shape_buffer(self.fonts, &text, &spans, style, inner_width, metrics);
        let graphemes = text.grapheme_indices(true).map(|(i, text)| i..i + text.len()).collect::<Vec<_>>();
        let starts = BidiParagraphs::new(&text)
            .map(|line| line.as_ptr().addr() - text.as_ptr().addr()).collect::<Vec<_>>();
        let mut height = f64::from(line_height);
        for run in buffer.layout_runs() {
            let top = inner_y + f64::from(run.line_top);
            let line_height = f64::from(run.line_height);
            height = height.max(f64::from(run.line_top) + line_height);
            let line_start = starts.get(run.line_i).copied().unwrap_or(0);
            let mut geometry = std::collections::BTreeMap::<usize, (Rect, bool)>::new();
            for glyph in run.glyphs {
                text_scene.glyphs.push(Glyph {
                    physical: glyph.physical((inner_x as f32, inner_y as f32 + run.line_y), 1.0),
                    color: glyph.color_opt.unwrap_or(style.color),
                });
                let start = line_start + glyph.start;
                let end = line_start + glyph.end;
                let first = graphemes.partition_point(|grapheme| grapheme.end <= start);
                let last = graphemes.partition_point(|grapheme| grapheme.start < end).max(first);
                if first == last {
                    continue;
                }
                let rtl = glyph.level.is_rtl();
                let advance = f64::from(glyph.w) / (last - first) as f64;
                for (i, index) in (first..last).enumerate() {
                    let offset = if rtl { last - first - i - 1 } else { i };
                    let bounds = rect(inner_x + f64::from(glyph.x) + advance * offset as f64, top, advance, line_height);
                    geometry.entry(index).and_modify(|(old, _)| {
                        old.left = old.left.min(bounds.left);
                        old.right = old.right.max(bounds.right);
                    }).or_insert((bounds, rtl));
                }
            }

            let mut clusters = Vec::with_capacity(geometry.len());
            for (index, (bounds, rtl)) in geometry {
                let bytes = &graphemes[index];
                let first = spans.partition_point(|span| span.bytes.end <= bytes.start);
                let last = spans.partition_point(|span| span.bytes.start < bytes.end);
                let (Some(first), Some(last)) = (spans.get(first), last.checked_sub(1).and_then(|index| spans.get(index)))
                    else { continue };
                let model = first.model.start..last.model.end;
                let preedit = first.preedit.as_ref().zip(last.preedit.as_ref()).map(|(first, last)| first.start..last.end);
                if first.object {
                    text_scene.nodes.push(NodeGeometry { from: model.start, to: model.end, rect: bounds });
                }
                if preedit.as_ref().zip(self.composition).is_some_and(|(range, composition)| {
                    !composition.selection.is_empty() && range.start < composition.selection.end && range.end > composition.selection.start
                }) {
                    text_scene.fill(bounds, SELECTION);
                }
                if let Some(background) = first.style.background {
                    text_scene.fill(bounds, background);
                }
                if first.style.underline {
                    text_scene.fill(Rect { top: bounds.bottom - 3.0, bottom: bounds.bottom - 2.0, ..bounds }, first.style.color);
                }
                if first.style.strikethrough {
                    text_scene.fill(rect(bounds.left, bounds.center().y, bounds.width(), 1.0), first.style.color);
                }
                clusters.push(Cluster { model, preedit, rect: bounds, rtl });
            }
            let line_from = clusters.iter().map(|cluster| cluster.model.start).min()
                .unwrap_or_else(|| boundary_at_byte(&spans, line_start, from));
            let line_to = clusters.iter().map(|cluster| cluster.model.end).max().unwrap_or(line_from);
            text_scene.lines.push(VisualLine {
                from: line_from,
                to: line_to,
                inside: node.from - self.base,
                rect: rect(inner_x, top, inner_width, line_height),
                clusters,
            });
        }

        let trailing_break = text.ends_with(['\n', '\r']) && text_scene.lines.last().is_some_and(|line| line.from < to);
        if text_scene.lines.is_empty() || trailing_break {
            let top = if trailing_break { inner_y + height } else { inner_y };
            let pos = if trailing_break { to } else { from };
            let height = f64::from(line_height);
            text_scene.lines.push(VisualLine {
                from: pos, to: pos, inside: node.from - self.base,
                rect: rect(inner_x, top, inner_width, height), clusters: Vec::new(),
            });
        }
        let height = text_scene.lines.last().map_or(height, |line| line.rect.bottom - inner_y) + 2.0 * padding;
        if code_block {
            let bounds = rect(x, y, width, height);
            scene.fill(bounds, CODE);
            scene.border(bounds, GRID);
        }
        scene.append(text_scene, 0.0);
        height
    }

    fn label(&mut self, text: &str, x: f64, y: f64, scene: &mut Scene) {
        let style = TextStyle { color: MUTED, ..TextStyle::default() };
        let mut buffer = Buffer::new(self.fonts, Metrics::new(15.0, 22.0));
        buffer.set_text(text, &style.attrs(), Shaping::Advanced, None);
        buffer.shape_until_scroll(self.fonts, false);
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                scene.glyphs.push(Glyph {
                    physical: glyph.physical((x as f32, y as f32 + run.line_y), 1.0), color: style.color,
                });
            }
        }
    }
}

fn task_checkbox(scene: &mut Scene, x: f64, y: f64, checked: bool) {
    let bounds = rect(x, y, 14.0, 14.0);
    scene.border(bounds, if checked { BLUE } else { MUTED });
    if checked {
        scene.fill(rect(x + 1.0, y + 1.0, 12.0, 12.0), BLUE);
        for (column, row) in [7.0, 8.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0].into_iter().enumerate() {
            scene.fill(rect(x + 3.0 + column as f64, y + row, 1.0, 2.0), PAPER);
        }
    }
}

fn child_node(item: &DisplayItem) -> Option<&DisplayNode> {
    if let DisplayItem::Node(node) = item { Some(node) } else { None }
}

fn collect_units(items: &[DisplayItem], base: usize, style: TextStyle, output: &mut Vec<Unit>) {
    for item in items {
        match item {
            DisplayItem::Text(run) => {
                let style = style.with_marks(&run.marks).with_decoration(&run.decorations);
                output.extend(run.text.iter().copied().enumerate().map(|(i, value)| Unit {
                    value, model: run.from + i - base..run.from + i + 1 - base, preedit: None, style, object: false,
                }));
            }
            DisplayItem::Node(node) if node.node.is_leaf() || node.node.is_atom() => {
                let style = style.with_marks(node.node.marks()).with_decoration(&node.decorations);
                let value = if node.node.type_name() == "hard_break" { b'\n' as u16 } else { 0x25c7 };
                output.push(Unit { value, model: node.from - base..node.to - base, preedit: None, style, object: value != b'\n' as u16 });
            }
            DisplayItem::Node(node) => collect_units(&node.children, base, style.with_decoration(&node.decorations), output),
            DisplayItem::Widget(_) => {}
        }
    }
}

fn boundary_at_byte(spans: &[TextSpan], byte: usize, fallback: usize) -> usize {
    let index = spans.partition_point(|span| span.bytes.end <= byte);
    spans.get(index).map(|span| span.model.start).or_else(|| spans.last().map(|span| span.model.end)).unwrap_or(fallback)
}

fn shape_buffer(
    fonts: &mut FontSystem, text: &str, spans: &[TextSpan], style: TextStyle,
    width: f64, metrics: Metrics,
) -> Buffer {
    let mut buffer = Buffer::new(fonts, metrics);
    buffer.set_size(Some(width as f32), None);
    let mut styled: Vec<(Range<usize>, TextStyle)> = Vec::new();
    for span in spans {
        if let Some((previous, _)) = styled.last_mut().filter(|(_, last)| *last == span.style) {
            previous.end = span.bytes.end;
        } else {
            styled.push((span.bytes.clone(), span.style));
        }
    }
    let runs = styled.into_iter().map(|(bytes, style)| (&text[bytes], style.attrs()));
    buffer.set_rich_text(runs, &style.attrs(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    buffer
}

impl SurfaceHost {
    fn positioned_block(&self, index: usize) -> PositionedBlock<'_> {
        let block = self.measure_block(index);
        PositionedBlock { from: self.position(index), origin_y: self.origin(index), scene: &block.scene }
    }

    fn blocks_at_pos(&self, pos: usize) -> impl Iterator<Item = PositionedBlock<'_>> {
        self.blocks_in_range(pos, pos)
    }

    fn blocks_in_range(&self, from: usize, to: usize) -> impl Iterator<Item = PositionedBlock<'_>> {
        (self.block_at_pos(from)..self.blocks.len()).take_while(move |&index| self.position(index) <= to)
            .map(|index| self.positioned_block(index))
    }

    pub(super) fn document_node_at(&self, pos: usize) -> Option<&Node> {
        let index = self.index.borrow().find(|sum| sum.units <= pos);
        let block = &self.blocks.get(index)?.source.node;
        let offset = pos - self.position(index);
        if offset == 0 { Some(block) } else { block.node_at(offset - 1) }
    }

    fn widget_art(
        &self, widget: &prosemirror_editor::DisplayWidget, fills: &mut Vec<Fill>,
        labels: &mut Vec<(Point, String)>,
    ) {
        let Some(bounds) = self.coords_at_pos(widget.pos, widget.side) else { return };
        let spec = widget.decoration.spec();
        if let Some(class) = spec["class"].as_str() {
            if class.contains("gapcursor") || class.contains("dropcursor") {
                fills.push(Fill { rect: rect(bounds.left, bounds.top, bounds.width().max(24.0), 2.0), color: BLUE });
            } else if class.contains("column-resize-handle") {
                fills.push(Fill { rect: rect(bounds.right - 2.0, bounds.top, 3.0, bounds.height().max(14.0)), color: BLUE });
            }
        }
        if let Some(text) = spec["text"].as_str() {
            labels.push((Point { x: bounds.left, y: bounds.top }, text.to_owned()));
        }
    }

    fn node_widgets(
        &self, node: &DisplayNode, fills: &mut Vec<Fill>, labels: &mut Vec<(Point, String)>,
    ) {
        for item in &node.children {
            match item {
                DisplayItem::Node(node) => self.node_widgets(node, fills, labels),
                DisplayItem::Widget(widget) => self.widget_art(widget, fills, labels),
                DisplayItem::Text(_) => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PositionedBlock<'a> {
    from: usize,
    origin_y: f64,
    scene: &'a Scene,
}

impl Layout for SurfaceHost {
    fn coords_at_pos(&self, pos: usize, side: i32) -> Option<Rect> {
        if pos > self.document_size {
            return None;
        }

        let mut alternate = None;
        let mut fallback = None;
        for block in self.blocks_at_pos(pos) {
            let local = pos - block.from;
            for line in &block.scene.lines {
                if local < line.from || local > line.to {
                    continue;
                }
                if line.clusters.is_empty() && local == line.from {
                    let empty = Some(translate(Rect { right: line.rect.left, ..line.rect }, 0.0, block.origin_y));
                    if side >= 0 {
                        return empty;
                    }
                    alternate = empty;
                }
                for cluster in &line.clusters {
                    let range = &cluster.model;
                    if (side >= 0 && local == range.start) || (side < 0 && local == range.end) {
                        return Some(translate(cluster.edge(side >= 0), 0.0, block.origin_y));
                    }
                    if local == range.start || local == range.end {
                        alternate = Some(translate(cluster.edge(local == range.start), 0.0, block.origin_y));
                    } else if range.start < local && local < range.end {
                        fallback = Some(translate(cluster.edge(side < 0), 0.0, block.origin_y));
                    }
                }
            }
            if fallback.is_none() {
                let geometry = block.scene.nodes.iter().filter(|node| node.from <= local && local <= node.to)
                    .min_by_key(|node| node.to - node.from);
                fallback = geometry.map(|node| {
                    let at_start = if local == node.from { true } else if local == node.to { false } else { side >= 0 };
                    let y = if at_start { node.rect.top } else { node.rect.bottom };
                    translate(Rect { top: y, bottom: y, ..node.rect }, 0.0, block.origin_y)
                });
            }
        }

        alternate.or(fallback).or_else(|| {
            (self.document_size == 0).then(|| rect(PAGE_INSET, PAGE_INSET, 0.0, 24.0))
        })
    }

    fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> {
        if !self.viewport()?.contains(point) {
            return None;
        }
        if self.blocks.is_empty() {
            return Some(PositionAtCoords { pos: 0, inside: None });
        }

        let index = self.block_at_y(point.y);
        let index = if index < self.blocks.len()
            && self.origin(index) + self.measure_block(index).height < point.y
        { index + 1 } else { index };
        let start = index.saturating_sub(1);
        let stop = (index + 2).min(self.blocks.len());
        for index in start..stop { self.measure_block(index); }
        let candidates: Vec<_> = (start..stop).map(|index| self.positioned_block(index)).collect();
        let line = candidates.iter().flat_map(|block| {
            block.scene.lines.iter().map(move |line| (block, line))
        }).min_by(|(left, a), (right, b)| {
            line_distance(a, left.origin_y, point).total_cmp(&line_distance(b, right.origin_y, point))
        });
        let bounds = candidates.iter().flat_map(|block| {
            block.scene.nodes.iter().filter_map(move |node| {
                let bounds = translate(node.rect, 0.0, block.origin_y);
                bounds.contains(point).then_some((block, node, bounds))
            })
        }).min_by_key(|(_, node, _)| node.to - node.from);
        if let Some((block, node, bounds)) = bounds.as_ref() {
            let rendered_text = block.scene.lines.iter().any(|line| {
                line.inside >= node.from && line.inside < node.to
            });
            if !rendered_text {
                let is_inline = bounds.height() > 16.0 && node.to - node.from == 1;
                let first_half = if is_inline { point.x < bounds.center().x } else { point.y < bounds.center().y };
                return Some(PositionAtCoords {
                    pos: block.from + if first_half { node.from } else { node.to },
                    inside: Some(block.from + node.from),
                });
            }
        }
        if let Some((block, line)) = line {
            let local = line.clusters.iter().min_by(|a, b| {
                x_distance(a.rect, point.x).total_cmp(&x_distance(b.rect, point.x))
            }).map_or(line.from, |cluster| {
                let start = (point.x < cluster.rect.center().x) ^ cluster.rtl;
                if start { cluster.model.start } else { cluster.model.end }
            });
            let inside = bounds.map_or(block.from + line.inside, |(block, node, _)| block.from + node.from);
            return Some(PositionAtCoords { pos: block.from + local, inside: Some(inside) });
        }
        Some(PositionAtCoords {
            pos: if point.y < f64::midpoint(PAGE_INSET, self.origin(self.blocks.len()) - BLOCK_GAP) {
                0
            } else { self.document_size },
            inside: None,
        })
    }

    fn line_boxes(&self, from: usize, to: usize) -> Vec<LineBox> {
        self.blocks_in_range(from, to)
            .flat_map(|block| block.scene.lines.iter().filter_map(move |line| {
                let start = block.from + line.from;
                let end = block.from + line.to;
                (start <= to && end >= from).then_some(LineBox {
                    from: start, to: end, rect: translate(line.rect, 0.0, block.origin_y),
                })
            })).collect()
    }

    fn block_bounds(&self, pos: usize) -> Option<Rect> {
        self.blocks_at_pos(pos).find_map(|block| {
            block.scene.nodes.iter().find(|node| node.from == pos - block.from)
                .map(|node| translate(node.rect, 0.0, block.origin_y))
        })
    }

    fn composition_caret(&self, composition: &CompositionDisplay) -> Option<Rect> {
        let caret = composition.selection.end;
        let mut closest = None;
        for block in self.blocks_at_pos(composition.from) {
            for cluster in block.scene.lines.iter().flat_map(|line| &line.clusters) {
                let Some(preedit) = cluster.preedit.as_ref() else { continue };
                for (start, offset) in [(true, preedit.start), (false, preedit.end)] {
                    let distance = caret.abs_diff(offset);
                    if closest.as_ref().is_none_or(|(previous, _)| distance < *previous) {
                        closest = Some((distance, translate(cluster.edge(start), 0.0, block.origin_y)));
                    }
                }
            }
        }
        closest.map(|(_, bounds)| bounds)
    }

    fn selection_rects(&self, from: usize, to: usize) -> Vec<Rect> {
        if from >= to {
            return Vec::new();
        }
        let mut rectangles = Vec::new();
        for block in self.blocks_in_range(from, to) {
            let base = block.from;
            for line in &block.scene.lines {
                let mut visible = line.clusters.iter().filter(|cluster| {
                    base + cluster.model.start < to && base + cluster.model.end > from
                }).map(|cluster| cluster.rect).collect::<Vec<_>>();
                visible.sort_by(|a, b| a.left.total_cmp(&b.left));
                let mut line_rect: Option<Rect> = None;
                for bounds in visible {
                    if let Some(previous) = line_rect.as_mut().filter(|previous| previous.right + 0.5 >= bounds.left) {
                        previous.right = previous.right.max(bounds.right);
                    } else if let Some(previous) = line_rect.replace(bounds) {
                        rectangles.push(translate(previous, 0.0, block.origin_y));
                    }
                }
                if let Some(bounds) = line_rect {
                    rectangles.push(translate(bounds, 0.0, block.origin_y));
                }
            }
        }
        rectangles
    }

    fn viewport_selection_rects(&self, from: usize, to: usize) -> Vec<Rect> {
        let viewport = self.measure_viewport();
        self.selection_rects(from.max(self.position(viewport.start)), to.min(self.position(viewport.end)))
    }
}

impl Host for SurfaceHost {
    fn incremental_display_list(&self) -> bool { true }

    fn update_layout(&mut self, frame: &DisplayList, changed: &BlockUpdate) {
        self.prepare_layout(frame, changed);
    }

    fn draw(&mut self, frame: &DisplayList, _changed: &BlockUpdate) {
        let mut marks = Vec::new();
        let mut labels = Vec::new();
        for widget in &frame.root_widgets {
            self.widget_art(widget, &mut marks, &mut labels);
        }
        for block in &frame.blocks {
            self.node_widgets(block, &mut marks, &mut labels);
        }

        let drop_line = self.drop_line.or_else(|| {
            frame.drag.as_ref().and_then(|drag| drag.over).and_then(|pos| self.coords_at_pos(pos, 1))
        });
        if let Some(line) = drop_line {
            let marker = if line.width() > 4.0 {
                Rect { bottom: line.top + 2.0, ..line }
            } else {
                rect(line.left, line.top, 2.0, line.height().max(22.0))
            };
            marks.push(Fill { rect: marker, color: BLUE });
        }

        let ghost = self.drag_ghost.clone().or_else(|| frame.drag.as_ref().zip(drop_line).and_then(|(drag, line)| {
            let node = self.document_node_at(drag.source)?;
            let text = node.text_content().chars().take(32).collect::<String>();
            let text = if text.is_empty() { node.type_name().to_owned() } else { text };
            Some((Point { x: line.left + 8.0, y: line.top + 8.0 }, text))
        }));
        if let Some((at, text)) = ghost {
            marks.push(Fill { rect: rect(at.x - 5.0, at.y - 2.0, (text.chars().count() as f64 * 8.0 + 14.0).min(280.0), 26.0),
                color: Color::rgba(215, 231, 249, 230) });
            labels.push((at, text));
        }

        let mut handles = self.touch_handles.clone();
        if frame.touch_handles {
            for (pos, side) in [(frame.selection.anchor, 1), (frame.selection.head, -1)] {
                if let Some(caret) = self.coords_at_pos(pos, side) {
                    handles.push(rect(caret.left - 4.0, caret.bottom - 2.0, 8.0, 9.0));
                }
                if frame.selection.anchor == frame.selection.head {
                    break;
                }
            }
        }

        let visible = self.measure_viewport();
        let origins: Vec<_> = visible.clone().map(|index| self.origin(index)).collect();
        let visible_blocks = self.blocks[visible].iter().zip(&origins)
            .map(|(block, &y)| (&block.measured.get().expect("measured visible block").scene, y));
        let mut label_scene = Scene::default();
        let fonts = self.fonts.get_mut();
        let mut shaper = Shaper { fonts, base: 0, composition: None };
        for (point, text) in labels {
            shaper.label(&text, point.x, point.y, &mut label_scene);
        }
        self.pixels.fill(PAPER.0 & 0x00ff_ffff);
        let mut canvas = Canvas { width: self.width as usize, height: self.height as usize, pixels: &mut self.pixels, scroll: self.scroll };
        for (scene, origin) in visible_blocks.clone() {
            for fill in &scene.fills {
                canvas.fill(translate(fill.rect, 0.0, origin), fill.color);
            }
        }
        for highlight in &frame.selection.highlights {
            canvas.fill(*highlight, SELECTION);
        }
        for (scene, origin) in visible_blocks {
            for glyph in &scene.glyphs {
                canvas.glyph(&mut self.raster, fonts, glyph, origin);
            }
        }
        if frame.focused {
            if let Some(caret) = frame.composition_caret {
                canvas.fill(Rect { right: caret.left + 2.0, ..caret }, BLUE);
            } else {
                for caret in &frame.selection.carets {
                    let bounds = caret.rect;
                    let bounds = if frame.selection.kind == SelectionKind::Gap {
                        rect(bounds.left, bounds.top, 26.0, 2.0)
                    } else {
                        Rect { right: bounds.left + 1.0, ..bounds }
                    };
                    canvas.fill(bounds, BLUE);
                }
            }
        }
        for item in marks {
            canvas.fill(item.rect, item.color);
        }
        for glyph in &label_scene.glyphs {
            canvas.glyph(&mut self.raster, fonts, glyph, 0.0);
        }
        for handle in handles {
            canvas.fill(handle, BLUE);
        }
    }

    fn read_clipboard(&mut self) -> Option<ClipboardContent> {
        self.clipboard.clone()
    }

    fn write_clipboard(&mut self, content: &ClipboardContent) {
        self.clipboard = Some(content.clone());
    }

    fn set_ime_candidate_rect(&mut self, rect: Option<Rect>) {
        self.candidate = rect;
    }

    fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor = cursor;
    }

    fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    fn viewport(&self) -> Option<Rect> {
        Some(rect(self.scroll.x, self.scroll.y, f64::from(self.width), f64::from(self.height)))
    }

    fn scroll_by(&mut self, delta: Point) {
        if delta.x.is_finite() {
            self.scroll.x += delta.x;
        }
        if delta.y.is_finite() {
            self.scroll.y += delta.y;
        }
        self.limit_scroll();
        self.measure_viewport();
        self.limit_scroll();
    }

    fn update_accessibility(&mut self, update: &AccessibilityTreeUpdate) {
        self.accessibility_state = None;
        self.accessibility = OnceCell::from(update.clone());
    }

    fn update_accessibility_state(&mut self, state: &EditorState) {
        self.accessibility_state = Some(state.clone());
        self.accessibility = OnceCell::new();
    }
}

struct Canvas<'a> {
    width: usize,
    height: usize,
    pixels: &'a mut [u32],
    scroll: Point,
}

impl Canvas<'_> {
    fn fill(&mut self, rect: Rect, color: Color) {
        let rect = translate(rect, -self.scroll.x, -self.scroll.y);
        let left = (rect.left.floor() as usize).min(self.width);
        let right = (rect.right.ceil() as usize).min(self.width);
        let top = (rect.top.floor() as usize).min(self.height);
        let bottom = (rect.bottom.ceil() as usize).min(self.height);
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            for pixel in &mut self.pixels[y * self.width + left..y * self.width + right] {
                *pixel = blend(*pixel, color);
            }
        }
    }

    fn glyph(&mut self, cache: &mut SwashCache, fonts: &mut FontSystem, glyph: &Glyph, dy: f64) {
        let x_offset = glyph.physical.x - self.scroll.x.round() as i32;
        let y_offset = glyph.physical.y + (dy - self.scroll.y).round() as i32;
        cache.with_pixels(fonts, glyph.physical.cache_key, glyph.color, |x, y, color| {
            let (x, y) = (x + x_offset, y + y_offset);
            if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
                let pixel = &mut self.pixels[y as usize * self.width + x as usize];
                *pixel = blend(*pixel, color);
            }
        });
    }
}

fn blend(background: u32, foreground: Color) -> u32 {
    let alpha = u32::from(foreground.a());
    if alpha == 255 {
        return foreground.0 & 0x00ff_ffff;
    }
    let channel = |shift: u32| {
        let fg = (foreground.0 >> shift) & 255;
        let bg = (background >> shift) & 255;
        ((fg * alpha + bg * (255 - alpha) + 127) / 255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect { left: x, top: y, right: x + width, bottom: y + height }
}

fn translate(rect: Rect, x: f64, y: f64) -> Rect {
    Rect { left: rect.left + x, right: rect.right + x, top: rect.top + y, bottom: rect.bottom + y }
}

fn x_distance(rect: Rect, x: f64) -> f64 {
    (rect.left - x).max(x - rect.right).max(0.0)
}

fn line_distance(line: &VisualLine, y: f64, point: Point) -> f64 {
    let vertical = (line.rect.top + y - point.y).max(point.y - line.rect.bottom - y).max(0.0);
    let horizontal = x_distance(line.rect, point.x);
    vertical.mul_add(vertical, horizontal * horizontal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prosemirror_model::{Fragment, Node, Schema};
    use prosemirror_state::Selection;
    use prosemirror_view::DecorationGroup;
    use serde_json::json;

    fn paragraph(schema: &Schema, text: &str) -> Node {
        let contents = Fragment::from_nodes(vec![schema.text(text, None)]);
        schema.node("paragraph", None, Some(contents), None).unwrap()
    }

    fn doc(schema: &Schema, children: Vec<Node>) -> Node {
        schema.node("doc", None, Some(Fragment::from_nodes(children)), None).unwrap()
    }

    fn frame(doc: &Node, selection: &Selection) -> DisplayList {
        let mut frame = DisplayList::unmeasured(doc, selection, &DecorationGroup::default());
        frame.focused = true;
        frame
    }

    fn style_document() -> Node {
        crate::setup::read_doc(&json!({"type":"doc", "content":[
            {"type":"paragraph", "content":[
                {"type":"text", "text":"a"},
                {"type":"text", "text":"b", "marks":[{"type":"strong"}]},
                {"type":"text", "text":"c", "marks":[{"type":"em"}]},
                {"type":"text", "text":"d", "marks":[{"type":"code"}]},
                {"type":"text", "text":"e", "marks":[{"type":"link", "attrs":{"href":"/guide"}}]},
                {"type":"text", "text":"f", "marks":[{"type":"strike"}]},
                {"type":"text", "text":"g", "marks":[{"type":"strong"}, {"type":"em"}]},
                {"type":"text", "text":"h", "marks":[{"type":"strong"}, {"type":"code"}]},
                {"type":"text", "text":"i", "marks":[{"type":"em"}, {"type":"code"}]},
                {"type":"text", "text":"j", "marks":[{"type":"strong"}, {"type":"em"}, {"type":"code"}]}
            ]},
            {"type":"heading", "attrs":{"level":2}, "content":[{"type":"text", "text":"Head"}]},
            {"type":"code_block", "content":[{"type":"text", "text":"Wi"}]}
        ]}))
    }

    fn resolved_test_runs() -> Vec<TextStyle> {
        let doc = style_document();
        let frame = frame(&doc, &Selection::cursor(1));
        let block = &frame.blocks[0];
        let mut units = Vec::new();
        collect_units(&block.children, block.from, TextStyle::for_block(block).0, &mut units);
        let (text, spans) = text_spans(&units);
        assert_eq!(text, "abcdefghij");
        spans.into_iter().map(|span| span.style).collect()
    }

    #[test]
    fn strong_mark_resolves_bold_weight() {
        let style = resolved_test_runs()[1];
        assert_eq!(style, TextStyle { bold: true, ..TextStyle::default() });
        assert_eq!(style.attrs().weight, Weight::BOLD);
    }

    #[test]
    fn em_mark_resolves_italic_slant() {
        let style = resolved_test_runs()[2];
        assert_eq!(style, TextStyle { italic: true, ..TextStyle::default() });
        assert_eq!(style.attrs().style, Style::Italic);
    }

    #[test]
    fn code_mark_resolves_monospace_and_background() {
        let style = resolved_test_runs()[3];
        assert_eq!(style, TextStyle { mono: true, background: Some(CODE), ..TextStyle::default() });
        assert_eq!(style.attrs().family, Family::Monospace);
    }

    #[test]
    fn link_mark_resolves_color_and_underline() {
        let style = resolved_test_runs()[4];
        assert_eq!(style, TextStyle { color: BLUE, underline: true, ..TextStyle::default() });
        assert_eq!(style.attrs().color_opt, Some(BLUE));
    }

    #[test]
    fn strike_mark_resolves_strikethrough() {
        assert_eq!(resolved_test_runs()[5], TextStyle { strikethrough: true, ..TextStyle::default() });
    }

    #[test]
    fn regular_and_combined_text_runs_preserve_styles() {
        let styles = resolved_test_runs();
        let plain = TextStyle::default();
        let bold = TextStyle { bold: true, ..plain };
        let code = TextStyle { mono: true, background: Some(CODE), ..plain };
        assert_eq!(styles[0], plain);
        assert_eq!(styles[6..], [
            TextStyle { italic: true, ..bold },
            TextStyle { bold: true, ..code },
            TextStyle { italic: true, ..code },
            TextStyle { bold: true, italic: true, ..code },
        ]);
    }

    fn assert_block_style(kind: &str, expected: TextStyle, font_size: f32, line_height: f32) {
        let doc = style_document();
        let frame = frame(&doc, &Selection::cursor(1));
        let block = frame.blocks.iter().find(|block| block.node.type_name() == kind).unwrap();
        let (style, metrics) = TextStyle::for_block(block);
        assert_eq!(style, expected);
        assert_eq!((metrics.font_size, metrics.line_height), (font_size, line_height));
    }

    #[test]
    fn paragraph_resolves_regular_prose() {
        assert_block_style("paragraph", TextStyle::default(), 16.0, 24.0);
    }

    #[test]
    fn heading_resolves_bold_larger_text() {
        assert_block_style("heading", TextStyle { bold: true, ..TextStyle::default() }, 26.0, 34.0);
    }

    #[test]
    fn code_block_resolves_monospace_text() {
        assert_block_style("code_block", TextStyle { mono: true, ..TextStyle::default() }, 16.0, 24.0);
    }

    #[test]
    fn shaped_glyphs_use_actual_bold_italic_and_monospace_faces() {
        let doc = style_document();
        let frame = frame(&doc, &Selection::cursor(1));
        let mut host = SurfaceHost::deterministic(500, 250);
        host.update_layout(&frame, &BlockUpdate::between(None, &doc));
        let paragraph = [
            "Inter-Regular", "Inter-Bold", "Inter-Italic", "JetBrainsMono-Regular",
            "Inter-Regular", "Inter-Regular", "Inter-BoldItalic", "JetBrainsMono-Bold",
            "JetBrainsMono-Italic", "JetBrainsMono-BoldItalic",
        ];
        let heading = ["Inter-Bold"; 4];
        let code = ["JetBrainsMono-Regular"; 2];
        for (block, expected) in host.blocks.iter().zip([paragraph.as_slice(), &heading, &code]) {
            let glyphs = &block.measured.get().unwrap().scene.glyphs;
            assert_eq!(glyphs.len(), expected.len());
            let fonts = host.fonts.borrow();
            for (glyph, expected) in glyphs.iter().zip(expected) {
                let face = fonts.db().face(glyph.physical.cache_key.font_id).unwrap();
                assert_eq!(face.post_script_name.as_str(), *expected);
                assert_eq!(face.weight.0, if expected.contains("Bold") { 700 } else { 400 });
                assert_eq!(face.style, if expected.contains("Italic") { Style::Italic } else { Style::Normal });
                assert_eq!(face.monospaced, expected.starts_with("JetBrainsMono"));
            }
        }
        let code = &frame.blocks[2];
        let start = host.coords_at_pos(code.content_from, 1).unwrap().left;
        let between = host.coords_at_pos(code.content_from + 1, 1).unwrap().left;
        let end = host.coords_at_pos(code.content_to, -1).unwrap().left;
        assert!(((between - start) - (end - between)).abs() < 0.01);
    }

    #[test]
    fn utf16_hit_testing_preserves_emoji_and_combining_graphemes() {
        let schema = prosemirror_schema_basic::schema();
        let doc = doc(schema, vec![paragraph(schema, "A😀e\u{301} Z")]);
        let frame = frame(&doc, &Selection::cursor(1));
        let mut host = SurfaceHost::new(480, 200);
        host.update_layout(&frame, &BlockUpdate::between(None, &doc));

        let emoji_start = host.coords_at_pos(2, 1).unwrap();
        let emoji_end = host.coords_at_pos(4, -1).unwrap();
        assert!(emoji_end.left > emoji_start.left);
        assert_eq!(host.coords_at_pos(3, -1), Some(emoji_start));
        assert_eq!(host.coords_at_pos(3, 1), Some(emoji_end));
        let combining_start = host.coords_at_pos(4, 1).unwrap();
        let combining_end = host.coords_at_pos(6, -1).unwrap();
        assert_eq!(host.coords_at_pos(5, -1), Some(combining_start));
        assert_eq!(host.coords_at_pos(5, 1), Some(combining_end));
        let hit = host.pos_at_coords(Point { x: (emoji_start.left + emoji_end.left) / 2.0, y: emoji_start.center().y }).unwrap();
        assert!(matches!(hit.pos, 2 | 4));
    }

    #[test]
    fn rtl_carets_follow_the_visual_geometry() {
        let schema = prosemirror_schema_basic::schema();
        let doc = doc(schema, vec![paragraph(schema, "אבג")]);
        let frame = frame(&doc, &Selection::text(1, 4));
        let mut host = SurfaceHost::new(480, 200);
        host.update_layout(&frame, &BlockUpdate::between(None, &doc));
        let first = host.coords_at_pos(1, 1).unwrap();
        let end = host.coords_at_pos(4, -1).unwrap();
        assert!(first.left > end.left);
        let highlights = host.selection_rects(1, 4);
        assert_eq!(highlights.len(), 1);
        assert!(highlights[0].width() > 0.0);
    }

    #[test]
    fn changing_an_early_block_reuses_unchanged_shaping_and_moves_suffix_positions() {
        let schema = prosemirror_schema_basic::schema();
        let suffix = paragraph(schema, "stable suffix");
        let before = doc(schema, vec![paragraph(schema, "short"), suffix.clone()]);
        let after = doc(schema, vec![paragraph(schema, "a changed paragraph that wraps in a narrower viewport"), suffix]);
        let mut host = SurfaceHost::deterministic(170, 120);
        host.update_layout(&frame(&before, &Selection::cursor(1)), &BlockUpdate::between(None, &before));
        let previous_y = host.origin(1);
        let previous_shaping = host.measure_block(1).scene.glyphs.as_ptr();
        host.update_layout(&frame(&after, &Selection::cursor(1)), &BlockUpdate::between(Some(&before), &after));
        assert_eq!(previous_shaping, host.measure_block(1).scene.glyphs.as_ptr());
        assert!(host.origin(1) > previous_y);
        assert_eq!(host.position(1), after.child(0).node_size());
        host.scroll_by(Point { x: 0.0, y: 1_000.0 });
        assert!(host.viewport().unwrap().top > 0.0);
        assert_eq!(host.document_point(Point::default()).y, host.viewport().unwrap().top);
    }

    #[test]
    fn large_documents_shape_visible_blocks_and_reuse_their_suffix_on_typing() {
        let schema = prosemirror_schema_basic::schema();
        let model = doc(schema, (0..1000).map(|_| paragraph(schema, "Unchanged text")).collect());
        let state = EditorState::create(prosemirror_state::EditorStateConfig {
            schema: schema.clone(),
            doc: Some(model),
            selection: Some(Selection::cursor(1)),
            stored_marks: None,
            plugins: Vec::new(),
        });
        let mut editor = prosemirror_editor::Editor::new(
            state, SurfaceHost::deterministic(500, 210), prosemirror_editor::EditorProps::default(),
        );
        let cached = editor.host().measure_block(1).scene.glyphs.as_ptr();
        let previous = editor.host().position(999);
        assert!(editor.host().blocks[999].measured.get().is_none());
        editor.handle(prosemirror_editor::Event::TextInput("x".into())).unwrap();
        assert_eq!(editor.host().position(999), previous + 1);
        assert_eq!(editor.host().measure_block(1).scene.glyphs.as_ptr(), cached);
        assert!(editor.host().blocks[999].measured.get().is_none());
        let mut select_all = editor.state().tr();
        select_all.set_selection(Selection::All);
        editor.dispatch(select_all).unwrap();
        assert!(editor.host().blocks[999].measured.get().is_none());
        assert!(editor.host().coords_at_pos(previous + 2, 1).unwrap().top > 10000.0);
        assert!(editor.host().blocks[999].measured.get().is_some());
    }

    #[test]
    fn paints_composition_and_writes_standard_rgb_png() {
        let schema = prosemirror_schema_basic::schema();
        let doc = doc(schema, vec![paragraph(schema, "abc")]);
        let mut frame = frame(&doc, &Selection::cursor(2));
        frame.composition = Some(CompositionDisplay { from: 2, to: 3, text: "Ω🙂".into(), selection: 3..3 });
        let mut host = SurfaceHost::deterministic(300, 100);
        let changed = BlockUpdate::between(None, &doc);
        host.update_layout(&frame, &changed);
        frame.measure(&host);
        assert!(frame.composition_caret.is_some());
        host.draw(&frame, &changed);
        assert!(host.pixels().iter().any(|pixel| *pixel != PAPER.0 & 0x00ff_ffff));
        let mut output = Vec::new();
        host.write_png(&mut output).unwrap();
        assert!(output.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(output.ends_with(b"\0\0\0\0IEND\xae\x42\x60\x82"));
        assert!(host.pixels().iter().all(|pixel| (pixel >> 24) == 0));
        host.update_layout(&frame, &BlockUpdate::default());
        host.draw(&frame, &BlockUpdate::default());
        let mut repeated = Vec::new();
        host.write_png(&mut repeated).unwrap();
        assert_eq!(repeated, output);
    }

    #[test]
    fn table_geometry_measures_explicit_widths_and_spanning_cells() {
        let mut nodes = prosemirror_schema_basic::nodes();
        nodes.extend(prosemirror_tables::table_nodes(prosemirror_tables::TableNodesOptions::default()));
        let schema = Schema::new(nodes, prosemirror_schema_basic::marks());
        let cell = |text, width| json!({"type":"table_cell", "attrs":{"colwidth":[width]},
            "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]});
        let mut spanning = cell("first", 120);
        spanning["attrs"]["rowspan"] = json!(2);
        let doc = schema.node_from_json(&json!({"type":"doc", "content":[{"type":"table", "content":[
            {"type":"table_row", "content":[spanning,cell("second",80)]},
            {"type":"table_row", "content":[cell("third",80)]}
        ]}]})).unwrap();
        let first_row = doc.child(0).child(0);
        let second = 2 + first_row.child(0).node_size();
        let third = 2 + first_row.node_size();
        let frame = frame(&doc, &Selection::cell(&doc, 2, third).unwrap());
        let mut host = SurfaceHost::deterministic(500, 250);
        host.update_layout(&frame, &BlockUpdate::between(None, &doc));
        let first = host.block_bounds(2).unwrap();
        let second = host.block_bounds(second).unwrap();
        let third = host.block_bounds(third).unwrap();
        assert_eq!(first.width(), 120.0);
        assert_eq!(second.width(), 80.0);
        assert_eq!(second.left, third.left);
        assert_eq!(second.bottom, third.top);
        assert_eq!(first.bottom, third.bottom);
        assert_eq!(host.block_bounds(0).unwrap().width(), 200.0);
    }
}
