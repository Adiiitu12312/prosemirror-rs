use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{build_doc, p, text, test_schema};
use prosemirror_view::{
    build_accessibility_tree, GoogleDocsParser, HandlePhysics, ImeHandler, SelectionHandles,
    ViewLayout,
};

#[test]
fn test_spike_hindi_gboard_conjuncts() {
    let handler = ImeHandler::new();
    // Devanagari text: "नमस्ते" = न + म + स + ् + त + े (6 code points, 3 grapheme clusters)
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("नमस्ते")])(inner)]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(19)), // End of word
        stored_marks: None,
        plugins: Vec::new(),
    });

    // Backspace should delete the full extended grapheme cluster ("स्ते"), not leave an isolated virama
    let tr = handler.delete_previous_grapheme_cluster(&state);
    assert!(tr.is_some(), "Gboard grapheme cluster deletion must succeed");
}

#[test]
fn test_spike_ios_selection_handles() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Touch selection text")])(inner)]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::text(1, 6)),
        stored_marks: None,
        plugins: Vec::new(),
    });

    let layout = ViewLayout::build(&state, 400.0);
    let physics = HandlePhysics::default();

    let handles = SelectionHandles::compute(&state, &layout, &physics, true, Some((50.0, 100.0)));
    assert!(handles.anchor_handle.is_some());
    assert!(handles.head_handle.is_some());
    assert!(handles.loupe_rect.is_some());

    // Apple HIG 44pt minimum touch target check
    let anchor = handles.anchor_handle.unwrap();
    assert_eq!(anchor.hit_target.width, 44.0);
    assert_eq!(anchor.hit_target.height, 44.0);
}

#[test]
fn test_spike_windows_ime_composition() {
    let mut handler = ImeHandler::new();
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("IME test: ")])(inner)]
        })
    }).node;

    let mut state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(11)),
        stored_marks: None,
        plugins: Vec::new(),
    });

    handler.handle_composition_start(&state);
    let tr_comp = handler.handle_composition_update(&state, "nihon");
    state = state.apply(&tr_comp);
    assert_eq!(state.doc.text_between(1, 16), "IME test: nihon");

    let tr_commit = handler.handle_composition_end(&state, "日本");
    state = state.apply(&tr_commit);
    assert_eq!(state.doc.text_between(1, 13), "IME test: 日本");
}

#[test]
fn test_spike_accesskit_screen_reader_tree() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![
                p(vec![text("First accessible paragraph")])(inner),
                p(vec![text("Second paragraph")])(inner),
            ]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(1)),
        stored_marks: None,
        plugins: Vec::new(),
    });

    let layout = ViewLayout::build(&state, 500.0);
    let tree_update = build_accessibility_tree(&state, &layout);

    assert_eq!(tree_update.root_id, 1);
    assert!(tree_update.nodes.len() >= 4); // Root + 2 Paragraphs + TextBoxes
}

#[test]
fn test_spike_google_docs_paste() {
    let gdocs_html = r#"<meta charset='utf-8'><b id="docs-internal-guid-abcdef-1234" style="font-weight:normal;"><span style="font-size:11pt;font-family:Arial;font-weight:700;font-style:italic;">Bold and Italic from Google Docs</span></b>"#;

    assert!(GoogleDocsParser::is_google_docs_html(gdocs_html));

    let slice = GoogleDocsParser::parse_clipboard_html(gdocs_html, test_schema());
    assert_eq!(slice.open_start, 1);
    assert_eq!(slice.open_end, 1);
    assert_eq!(slice.content.child_count(), 1);

    let para = slice.content.child(0);
    let text_node = para.child(0);
    assert_eq!(text_node.text().unwrap(), "Bold and Italic from Google Docs");

    let marks = text_node.marks();
    assert!(marks.iter().any(|m| m.type_name == "strong"));
    assert!(marks.iter().any(|m| m.type_name == "em"));
}
