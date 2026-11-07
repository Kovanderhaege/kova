use crate::input::{self, TextInput};
use gpui::{prelude::*, *};
use std::ops::Range;

struct Spike {
    input: Entity<TextInput>,
    cache: kova_client_core::Cache,
    status: String,
}
impl Render for Spike {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x111827))
            .text_color(rgb(0xe5e7eb))
            .p_4()
            .gap_3()
            .child(div().text_xl().child("Kova / Windows — prototype local"))
            .child("10 000 lignes virtualisées · aucun message envoyé")
            .child(
                uniform_list(
                    "timeline",
                    10_000,
                    cx.processor(|_, range: Range<usize>, _, _| {
                        range
                            .map(|i| {
                                div()
                                    .h(px(30.))
                                    .px_2()
                                    .child(format!("Message de démonstration {}", i + 1))
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            )
            .child(self.input.clone())
            .child(
                div()
                    .id("save-draft")
                    .cursor_pointer()
                    .p_2()
                    .bg(rgb(0x334155))
                    .child("Enregistrer le brouillon")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.status = match this
                            .cache
                            .save_draft("spike", this.input.read(cx).content.as_ref())
                        {
                            Ok(()) => "Brouillon enregistré localement".into(),
                            Err(e) => format!("Échec : {e}"),
                        };
                        cx.notify();
                    })),
            )
            .child(self.status.clone())
            .child("Ctrl+A/C/V/X · flèches · sélection · Unicode / IME à vérifier sur Windows")
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        input::bind_keys(cx);
        let directory =
            std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
                .join("Kova")
                .join("spike");
        std::fs::create_dir_all(&directory).expect("create cache directory");
        let cache =
            kova_client_core::Cache::open(&directory.join("cache.sqlite")).expect("open cache");
        let draft = cache.draft("spike").expect("read draft");
        let bounds = Bounds::centered(None, size(px(900.), px(650.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("Kova — Windows spike".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    let input =
                        cx.new(|cx| TextInput::new(cx, "Votre brouillon local…", &draft, false));
                    cx.new(|_| Spike {
                        input,
                        cache,
                        status: "Prototype : notifications et packaging signé non validés".into(),
                    })
                },
            )
            .expect("open GPUI window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.input.focus_handle(cx));
                cx.activate(true);
            })
            .unwrap();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
