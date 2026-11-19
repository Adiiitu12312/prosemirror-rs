//! DOM projection for the menu and icon rendering in prosemirror-menu 1.3.0.

use prosemirror_view::browser::render_dom;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, Node, ShadowRoot};

use crate::{IconSpec, MenuControl, MenuControlKind, MenuDisplay, MenuGroup, MenuPath};

const BUTTON: &str = "data-pm-menu-button";
const PATH: &str = "data-pm-menu-path";
const SVG: &str = "http://www.w3.org/2000/svg";
const XLINK: &str = "http://www.w3.org/1999/xlink";

pub(super) struct MenuDom {
    groups: Vec<GroupDom>,
}

struct GroupDom {
    separator: Option<HtmlElement>,
    controls: Vec<ControlDom>,
}

struct ControlDom {
    path: MenuPath,
    shell: HtmlElement,
    container: HtmlElement,
    button: HtmlElement,
    popup: Option<HtmlElement>,
    list: Option<HtmlElement>,
    children: Vec<ControlDom>,
    display: String,
}

impl MenuDom {
    pub fn new(document: &Document, root: &Node, menu: &HtmlElement, groups: &[MenuGroup]) -> Result<Self, JsValue> {
        let mut dom_groups = Vec::with_capacity(groups.len());
        for group in groups {
            let separator = if dom_groups.is_empty() {
                None
            } else {
                let separator = element(document, "span", "ProseMirror-menuseparator")?;
                separator.set_attribute("role", "separator")?;
                menu.append_child(&separator)?;
                Some(separator)
            };
            let mut controls = Vec::with_capacity(group.items.len());
            for item in &group.items {
                let control = ControlDom::new(document, root, item, true)?;
                menu.append_child(&control.shell)?;
                controls.push(control);
            }
            dom_groups.push(GroupDom { separator, controls });
        }
        Ok(Self { groups: dom_groups })
    }

    pub fn update(&self, groups: &[MenuGroup]) -> Result<(), JsValue> {
        for (dom, group) in self.groups.iter().zip(groups) {
            if let Some(separator) = &dom.separator {
                show(separator, group.separator_before)?;
            }
            for (control, item) in dom.controls.iter().zip(&group.items) {
                control.update(item)?;
            }
        }
        Ok(())
    }

    pub fn button(&self, path: &[usize]) -> Option<&HtmlElement> {
        // Projection filters empty groups. Paths use the original group index.
        self.groups.iter().flat_map(|group| &group.controls)
            .find_map(|control| control.find(path)).map(|control| &control.button)
    }
}

impl ControlDom {
    fn new(document: &Document, root: &Node, control: &MenuControl, top: bool) -> Result<Self, JsValue> {
        let (tag, class) = if top { ("span", "ProseMirror-menuitem") }
            else { ("li", "ProseMirror-menu-dropdown-item") };
        let shell = element(document, tag, class)?;
        if !top {
            shell.set_attribute("role", "menuitem")?;
            shell.set_tab_index(-1);
        }
        let path = control.path.iter().map(usize::to_string).collect::<Vec<_>>().join(".");
        shell.set_attribute(PATH, &path)?;
        let button = match &control.display {
            MenuDisplay::Label(label) => {
                let button = element(document, "button", "")?;
                button.set_text_content(Some(label));
                button
            }
            MenuDisplay::Icon(icon) => render_icon(document, root, icon)?,
            MenuDisplay::Custom(dom) => {
                let custom = render_dom(document, dom)?;
                match custom.dyn_into::<HtmlElement>() {
                    Ok(element) => element,
                    Err(custom) => {
                        let button = element(document, "button", "")?;
                        button.append_child(&custom)?;
                        button
                    }
                }
            }
        };
        button.set_attribute(BUTTON, &path)?;
        if let Some(title) = &control.title {
            button.set_title(title);
        }
        if let Some(class) = &control.class {
            add_class(&button, class, control.kind)?;
        }
        if let Some(css) = &control.css {
            let current = button.style().css_text();
            button.style().set_css_text(&format!("{current}{css}"));
        }

        let (container, popup, list, children) = if control.kind == MenuControlKind::Item {
            (button.clone(), None, None, Vec::new())
        } else {
            let (wrap_class, button_class, popup_class) = match control.kind {
                MenuControlKind::Dropdown => (
                    "ProseMirror-menu-dropdown-wrap", "ProseMirror-menu-dropdown", "ProseMirror-menu-dropdown-menu",
                ),
                MenuControlKind::Submenu => (
                    "ProseMirror-menu-submenu-wrap", "ProseMirror-menu-submenu-label", "ProseMirror-menu-submenu",
                ),
                MenuControlKind::Item => unreachable!(),
            };
            let wrap = element(document, "div", wrap_class)?;
            let popup = element(document, "div", popup_class)?;
            if control.kind == MenuControlKind::Dropdown {
                if let Some(class) = &control.class {
                    add_class(&popup, class, control.kind)?;
                }
            }
            button.class_list().add_1(button_class)?;
            button.set_attribute("aria-haspopup", "menu")?;
            wrap.append_child(&button)?;
            let list = element(document, "ul", "")?;
            list.set_attribute("role", "menu")?;
            let mut children = Vec::with_capacity(control.children.len());
            for child in &control.children {
                let child = Self::new(document, root, child, false)?;
                list.append_child(&child.shell)?;
                children.push(child);
            }
            popup.append_child(&list)?;
            if control.kind == MenuControlKind::Submenu {
                wrap.append_child(&popup)?;
            }
            (wrap, Some(popup), Some(list), children)
        };
        shell.append_child(&container)?;
        let display = container.style().get_property_value("display")?;
        Ok(Self { path: control.path.clone(), shell, container, button, popup, list, children, display })
    }

    fn find(&self, path: &[usize]) -> Option<&Self> {
        if self.path == path { return Some(self) }
        if path.starts_with(&self.path) {
            return self.children.get(path[self.path.len()])?.find(path);
        }
        None
    }

    fn update(&self, control: &MenuControl) -> Result<(), JsValue> {
        show(&self.shell, control.state.visible)?;
        self.container.style().set_property("display", if control.state.visible { &self.display } else { "none" })?;
        self.button.set_tab_index(control.tab_index.into());
        if let Some(disabled) = control.aria_disabled {
            class(&self.button, "ProseMirror-menu-disabled", disabled)?;
            self.button.set_attribute("aria-disabled", if disabled { "true" } else { "false" })?;
        }
        if let Some(active) = control.aria_pressed {
            class(&self.button, "ProseMirror-menu-active", active)?;
            self.button.set_attribute("aria-pressed", if active { "true" } else { "false" })?;
        }
        if let (Some(open), Some(popup)) = (control.aria_expanded, &self.popup) {
            self.button.set_attribute("aria-expanded", if open { "true" } else { "false" })?;
            match control.kind {
                MenuControlKind::Dropdown if open && popup.parent_node().is_none() => {
                    self.container.append_child(popup)?;
                    if let Some(list) = &self.list {
                        controls_element(&self.button, Some(list));
                    }
                }
                MenuControlKind::Dropdown if !open && popup.parent_node().is_some() => {
                    popup.remove();
                    controls_element(&self.button, None);
                }
                MenuControlKind::Submenu => {
                    class(&self.container, "ProseMirror-menu-submenu-wrap-active", open)?;
                    controls_element(&self.button, if open { self.list.as_ref() } else { None });
                }
                _ => (),
            }
        }
        for (child, item) in self.children.iter().zip(&control.children) {
            child.update(item)?;
        }
        Ok(())
    }
}

pub(super) fn control_path(node: &Element, button_only: bool) -> Option<MenuPath> {
    let attribute = if button_only { BUTTON } else { PATH };
    let closest = node.closest(&format!("[{attribute}]")).ok().flatten()?;
    parse_path(&closest.get_attribute(attribute)?)
}

fn parse_path(path: &str) -> Option<MenuPath> {
    path.split('.').map(|part| part.parse().ok()).collect()
}

pub(super) fn element(document: &Document, tag: &str, class: &str) -> Result<HtmlElement, JsValue> {
    let element = document.create_element(tag)?.dyn_into::<HtmlElement>().map_err(JsValue::from)?;
    if !class.is_empty() {
        element.set_class_name(class);
    }
    Ok(element)
}

pub(super) fn show(element: &HtmlElement, show: bool) -> Result<(), JsValue> {
    element.style().set_property("display", if show { "" } else { "none" })
}

fn class(element: &HtmlElement, name: &str, has_class: bool) -> Result<(), JsValue> {
    element.class_list().toggle_with_force(name, has_class)?;
    Ok(())
}

fn add_class(element: &HtmlElement, classes: &str, kind: MenuControlKind) -> Result<(), JsValue> {
    if kind == MenuControlKind::Item {
        element.class_list().add_1(classes)?;
    } else {
        for name in classes.split_whitespace() {
            element.class_list().add_1(name)?;
        }
    }
    Ok(())
}

pub(super) fn controls_element(element: &HtmlElement, controlled: Option<&HtmlElement>) {
    let controls = js_sys::Array::new();
    if let Some(controlled) = controlled {
        controls.push(controlled.as_ref());
    }
    let _ = js_sys::Reflect::set(element.as_ref(), &JsValue::from_str("ariaControlsElements"), controls.as_ref());
}

fn render_icon(document: &Document, root: &Node, icon: &IconSpec) -> Result<HtmlElement, JsValue> {
    let IconSpec::Svg { path, width, height } = icon else {
        return render_dom(document, &icon.to_dom())?.dyn_into().map_err(JsValue::from);
    };
    let button = element(document, "button", "ProseMirror-icon")?;
    let name = icon.symbol_id().expect("SVG icon has a symbol name");
    if get_element(root, document, &name).is_none() {
        let collection = match get_element(root, document, "ProseMirror-icon-collection") {
            Some(collection) => collection,
            None => {
                let collection = document.create_element_ns(Some(SVG), "svg")?;
                collection.set_id("ProseMirror-icon-collection");
                collection.set_attribute("style", "display: none")?;
                let top = if root.node_type() == Node::DOCUMENT_NODE {
                    document.body().map_or_else(|| root.clone(), Into::into)
                } else {
                    root.clone()
                };
                top.insert_before(&collection, top.first_child().as_ref())?;
                collection
            }
        };
        let symbol = document.create_element_ns(Some(SVG), "symbol")?;
        symbol.set_id(&name);
        symbol.set_attribute("viewBox", &format!("0 0 {width} {height}"))?;
        let data = document.create_element_ns(Some(SVG), "path")?;
        data.set_attribute("d", path)?;
        symbol.append_child(&data)?;
        collection.append_child(&symbol)?;
    }
    let svg = document.create_element_ns(Some(SVG), "svg")?;
    let ratio = f64::from(*width) / f64::from((*height).max(1));
    svg.set_attribute("style", &format!("width: {ratio}em"))?;
    let reference = document.create_element_ns(Some(SVG), "use")?;
    let url = document.url()?;
    let base = url.split('#').next().unwrap_or_default();
    reference.set_attribute_ns(Some(XLINK), "href", &format!("{base}#{name}"))?;
    svg.append_child(&reference)?;
    button.append_child(&svg)?;
    Ok(button)
}

fn get_element(root: &Node, document: &Document, id: &str) -> Option<Element> {
    if let Some(shadow) = root.dyn_ref::<ShadowRoot>() {
        shadow.query_selector(&format!("#{id}")).ok().flatten()
    } else {
        document.get_element_by_id(id)
    }
}
