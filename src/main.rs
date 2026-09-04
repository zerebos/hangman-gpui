//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use hangman_gpui::ui::{ChangeWord, HangmanView, KEY_CONTEXT, OpenWordList};

fn main() {
    // The bundled Lucide icons; gpui-kit's title bar buttons need them.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);

    app.run(|cx| {
        // Must run before anything touches a gpui-kit component.
        gpui_kit::init(cx);

        // The original's Game menu accelerators, minus the menu bar.
        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenWordList, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-n", ChangeWord, Some(KEY_CONTEXT)),
        ]);

        // `WindowBounds::centered` needs an `&App`, which the async block below
        // does not have, so build the options out here.
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(900.), px(560.)), cx)),
            window_min_size: Some(size(px(720.), px(480.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Hangman!".into()),
                ..TitleBar::title_bar_options()
            }),
            ..TitleBar::window_options()
        };

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| HangmanView::new(window, cx));
                // Nothing receives key events until something is focused.
                let handle = view.focus_handle(cx);
                window.defer(cx, move |window, cx| handle.focus(window, cx));
                // Every gpui-kit window's first-level view must be a `Root`.
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the Hangman window");
        })
        .detach();
    });
}
