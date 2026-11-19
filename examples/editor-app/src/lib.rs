#![cfg(not(target_arch = "wasm32"))]

mod benchmark;
mod desktop;
mod replay;
mod setup;
pub mod surface;
mod system_clipboard;

use std::{env, error::Error, ffi::OsStr, path::Path};

use prosemirror_editor::{Editor, Event};
use prosemirror_keymap::Platform;

use surface::SurfaceHost;

pub fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => desktop::run(),
        [flag, input] if flag == OsStr::new("--replay") => replay::run(Path::new(input), None),
        [flag, input, png, output]
            if flag == OsStr::new("--replay") && png == OsStr::new("--png") =>
        {
            replay::run(Path::new(input), Some(Path::new(output)))
        }
        [flag, path] if flag == OsStr::new("--snapshot") => {
            let platform = Platform::Other;
            let state = setup::initialize(setup::sample(), None);
            let mut editor = Editor::with_platform(
                state, SurfaceHost::deterministic(900, 760), setup::props(platform), platform,
            );
            editor.handle(Event::Focus)?;
            editor.host().save_png(Path::new(path))
        }
        [flag, host] if flag == OsStr::new("--bench") && host == OsStr::new("core") => {
            benchmark::run_core()
        }
        [flag, host] if flag == OsStr::new("--bench") && host == OsStr::new("surface") => {
            benchmark::run_surface()
        }
        _ => Err("usage: editor-app [--replay INPUT [--png OUTPUT] | --snapshot PNG | --bench core|surface]".into()),
    }
}
