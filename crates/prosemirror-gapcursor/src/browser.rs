use std::rc::Rc;

use prosemirror_keymap::{KeyEvent, Modifiers, Platform};
use prosemirror_state::Transaction;
use prosemirror_view::browser::{
    BrowserView, BrowserViewProps, TextblockDirection, ViewEventHandler, ViewportPoint, WidgetView,
    WidgetViewFactory,
};
use wasm_bindgen::JsCast;
use web_sys::{Event, InputEvent};

use crate::{gap_cursor, Arrow, GapCursorView};

impl GapCursorView for BrowserView {
    fn editable(&self) -> bool {
        self.editable()
    }

    fn end_of_textblock(&self, arrow: Arrow) -> bool {
        let direction = match arrow {
            Arrow::Left => TextblockDirection::Left,
            Arrow::Right => TextblockDirection::Right,
            Arrow::Up => TextblockDirection::Up,
            Arrow::Down => TextblockDirection::Down,
        };
        self.end_of_textblock(direction, None)
    }

    fn node_at_point(&self, x: f64, y: f64) -> Option<usize> {
        self.pos_at_coords(ViewportPoint { left: x, top: y })?.inside
    }
}

/// Browser plugin props for the gap cursor. Load [`crate::CSS`] to show the widget.
pub fn browser_plugin(platform: Platform, widget_id: u64) -> BrowserViewProps {
    let plugin = Rc::new(gap_cursor::<BrowserView>(platform, widget_id));
    let decorations = Rc::clone(&plugin);
    let selections = Rc::clone(&plugin);
    let clicks = Rc::clone(&plugin);
    let input = Rc::clone(&plugin);
    let widget: WidgetViewFactory = Rc::new(|document, _, _, _| {
        let element = document.create_element("div")?;
        element.set_class_name("ProseMirror-gapcursor");
        Ok(WidgetView::new(element))
    });
    let beforeinput: ViewEventHandler<Event> = Rc::new(move |view, event| {
        event.dyn_ref::<InputEvent>().is_some_and(|event| {
            input.before_input(&view.state(), &mut |tr| dispatch(view, tr), &event.input_type())
        })
    });
    BrowserViewProps {
        decorations: vec![Rc::new(move |state| decorations.decorations(state))],
        widgets: [(widget_id, widget)].into(),
        create_selection_between: Some(Rc::new(move |_, anchor, head| {
            selections.create_selection_between(anchor, head)
        })),
        handle_click: Some(Rc::new(move |view, pos, event| {
            clicks.handle_click(&view.state(), &mut |tr| dispatch(view, tr), view, pos,
                f64::from(event.client_x()), f64::from(event.client_y()))
        })),
        handle_key_down: Some(Rc::new(move |view, event| {
            let key = event.key();
            plugin.handle_keydown(&view.state(), &mut |tr| dispatch(view, tr), view, KeyEvent {
                key: &key,
                key_code: u16::try_from(event.key_code()).ok(),
                modifiers: Modifiers {
                    alt: event.alt_key(), ctrl: event.ctrl_key(), meta: event.meta_key(), shift: event.shift_key(),
                },
                legacy_ie: false,
            })
        })),
        handle_dom_events: [("beforeinput".into(), beforeinput)].into(),
        ..Default::default()
    }
}

fn dispatch(view: &BrowserView, transaction: Transaction) {
    if let Err(error) = view.dispatch(transaction) {
        web_sys::console::error_1(&error);
    }
}
