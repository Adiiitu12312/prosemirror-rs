//! The desktop clipboard owns all representations of one selection together.

use clipboard_rs::{Clipboard, ClipboardContent as SystemContent, ClipboardContext, ContentFormat};
use prosemirror_editor::ClipboardContent;

#[cfg(target_os = "macos")]
pub(crate) const NATIVE_FORMAT: &str = "org.prosemirror.native-slice";
#[cfg(not(target_os = "macos"))]
pub(crate) const NATIVE_FORMAT: &str = "application/x-prosemirror-native-slice";

#[cfg(target_os = "linux")]
const X11_TEXT_FORMATS: &[&str] = &["text/plain;charset=utf-8", "text/plain;charset=UTF-8", "text/plain", "STRING"];

#[derive(Default)]
pub(crate) struct SystemClipboard {
    context: Option<ClipboardContext>,
    unavailable: Option<ClipboardContent>,
}

impl SystemClipboard {
    fn context(&mut self) -> Option<&ClipboardContext> {
        if self.context.is_none() {
            self.context = ClipboardContext::new().ok();
        }
        self.context.as_ref()
    }

    pub(crate) fn read(&mut self) -> Option<ClipboardContent> {
        let Some(clipboard) = self.context() else { return self.unavailable.clone() };
        let Ok(formats) = clipboard.get(&[
            ContentFormat::Text,
            ContentFormat::Html,
            ContentFormat::Other(NATIVE_FORMAT.into()),
        ]) else { return self.unavailable.clone() };
        let mut content = ClipboardContent::default();
        for format in formats {
            match format {
                SystemContent::Text(text) => content.plain_text = text,
                SystemContent::Html(html) if !html.is_empty() => content.html = Some(html),
                SystemContent::Other(name, bytes) if name == NATIVE_FORMAT => content.internal = Some(bytes),
                _ => {}
            }
        }
        #[cfg(target_os = "linux")]
        if content.plain_text.is_empty() && matches!(clipboard, ClipboardContext::X11(_)) {
            content.plain_text = x11_text(clipboard).unwrap_or_default();
        }
        self.unavailable = None;
        (!content.plain_text.is_empty() || content.html.is_some() || content.internal.is_some()).then_some(content)
    }

    pub(crate) fn write(&mut self, content: &ClipboardContent) {
        let written = self.context().is_some_and(|clipboard| {
            let mut formats = vec![SystemContent::Text(content.plain_text.clone())];
            #[cfg(target_os = "linux")]
            if matches!(clipboard, ClipboardContext::X11(_)) {
                for &format in &X11_TEXT_FORMATS[..3] {
                    formats.push(SystemContent::Other(format.into(), content.plain_text.as_bytes().to_vec()));
                }
            }
            if let Some(html) = &content.html {
                formats.push(SystemContent::Html(html.clone()));
            }
            if let Some(bytes) = &content.internal {
                formats.push(SystemContent::Other(NATIVE_FORMAT.into(), bytes.clone()));
            }
            clipboard.set(formats).is_ok()
                || clipboard.set_text(content.plain_text.clone()).is_ok()
        });
        self.unavailable = (!written).then(|| content.clone());
    }
}

#[cfg(target_os = "linux")]
fn x11_text(clipboard: &ClipboardContext) -> Option<String> {
    let available = clipboard.available_formats().ok()?;
    X11_TEXT_FORMATS.iter().copied().filter(|format| available.iter().any(|name| name == format))
        .find_map(|format| {
            let bytes = clipboard.get_buffer(format).ok()?;
            if format == "STRING" {
                return Some(bytes.into_iter().map(char::from).collect());
            }
            String::from_utf8(bytes).ok()
        })
}
