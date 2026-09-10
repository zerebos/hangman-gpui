//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.

use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Root, Theme, TitleBar};
use gpui_kit::*;

use hangman_gpui::settings::Settings;
use hangman_gpui::ui::{
    ChangeWord, HangmanView, Hint, KEY_CONTEXT, MIN_WINDOW_SIZE, OpenWordList, window_bounds,
};

fn main() {
    // The bundled Lucide icons; gpui-kit's title bar buttons need them.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);

    app.run(|cx| {
        // Must run before anything touches a gpui-kit component.
        gpui_kit::init(cx);

        // What the last run left behind: the theme, the window's place on the
        // desktop and the difficulty. A first launch, or a file that cannot be
        // read, gives the defaults instead — a dark, centred, Easy game.
        let settings = Settings::load();

        // `gpui_kit::init` installs the light palette, so the saved choice has
        // to be applied after it, not before. `Theme` is a GPUI global, so this
        // one call restyles every component. Swap it for
        // `Theme::sync_system_appearance(None, cx)` to follow the desktop
        // instead — the in-window toggle overrides either way.
        Theme::change(settings.theme, None, cx);

        // The play column is the only thing in this window that scrolls, and
        // with the default mode there was nothing to say so: the bar appears
        // once you are already scrolling, which is no use to someone who
        // cannot tell there is more below. `Always` draws it whenever the
        // content overflows and nothing at all when it fits, so it is a
        // "there is more" marker rather than furniture. This overrides
        // `theme::init`'s `sync_scrollbar_appearance`, which follows the
        // desktop's auto-hide preference — hence after `gpui_kit::init`, like
        // the theme itself. The mode survives `Theme::change`, so the
        // light/dark toggle does not undo it.
        Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);

        // The original's Game menu accelerators, minus the menu bar.
        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenWordList, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-n", ChangeWord, Some(KEY_CONTEXT)),
            // Not one of the original's — it had no hints — but it sits with
            // them: `on_key_down` lets any chord through rather than guessing
            // its letter, so Ctrl+H costs a guess and a bare H does not.
            KeyBinding::new("ctrl-h", Hint, Some(KEY_CONTEXT)),
        ]);

        // Restoring the saved bounds needs an `&App` — for the displays that
        // exist now, and for `WindowBounds::centered` when there is nothing to
        // restore — which the async block below does not have, so build the
        // options out here.
        let options = WindowOptions {
            window_bounds: Some(window_bounds(&settings, cx)),
            window_min_size: Some(MIN_WINDOW_SIZE),
            titlebar: Some(TitlebarOptions {
                title: Some("Hangman!".into()),
                ..TitleBar::title_bar_options()
            }),
            ..TitleBar::window_options()
        };

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| HangmanView::new(settings, window, cx));
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
