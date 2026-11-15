//! Navigable desktop shell; all I/O stays on the background executor.
use crate::{
    identity_ui::IdentityWindow,
    input::{self, TextInput},
    workspace_service::{self as service, Choice, Snapshot, Target},
};
use gpui::{prelude::*, *};
use std::collections::BTreeMap;

#[derive(Clone)]
enum Action {
    Refresh,
    Settings,
    Community(String),
    Select(Target, String),
    Member(String, String),
    Send,
    Retry,
    Sync,
    AutoSync,
    Earlier,
}

#[derive(Default)]
struct Drafts(Vec<(String, String, Target, String)>);

impl Drafts {
    fn save(&mut self, profile: &str, origin: &str, target: &Target, body: String) {
        if let Some(entry) = self
            .0
            .iter_mut()
            .find(|entry| entry.0 == profile && entry.1 == origin && &entry.2 == target)
        {
            entry.3 = body;
        } else {
            self.0
                .push((profile.into(), origin.into(), target.clone(), body));
        }
    }

    fn get(&self, profile: &str, origin: &str, target: Option<&Target>) -> String {
        self.0
            .iter()
            .find(|entry| entry.0 == profile && entry.1 == origin && Some(&entry.2) == target)
            .map(|entry| entry.3.clone())
            .unwrap_or_default()
    }
}

struct Workspace {
    profile: Entity<TextInput>,
    origin: Entity<TextInput>,
    composer: Entity<TextInput>,
    settings: Entity<IdentityWindow>,
    show_settings: bool,
    snapshot: Snapshot,
    community: Option<String>,
    member: Option<String>,
    target: Option<Target>,
    title: String,
    scope: (String, String),
    drafts: Drafts,
    buttons: BTreeMap<String, FocusHandle>,
    limit: usize,
    busy: bool,
    status: String,
    subscription: Option<crate::chat_service::Subscription>,
    timeline_loading: bool,
    operation_error: Option<String>,
    revision: u64,
}

impl Workspace {
    fn follow_selection(&mut self) {
        self.revision += 1;
        self.subscription = match (&self.target, self.show_settings) {
            (Some(Target::Channel(id)), false) => Some(crate::chat_service::Subscription::start(
                self.scope.0.clone(),
                self.scope.1.clone(),
                id.clone(),
            )),
            _ => None,
        };
    }

    fn start_updates(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |entity, cx| {
            let mut elapsed = 0;
            loop {
                Timer::after(std::time::Duration::from_secs(1)).await;
                elapsed += 1;
                let poll_dm = elapsed >= 30;
                if poll_dm {
                    elapsed = 0;
                }
                let active = entity
                    .update(cx, |this, cx| {
                        if this.busy || this.show_settings {
                            return;
                        }
                        let received = this.subscription.as_ref().and_then(|subscription| {
                            subscription
                                .latest
                                .lock()
                                .ok()
                                .and_then(|mut slot| slot.take())
                        });
                        if let Some(status) = received {
                            // The subscription owns transport/reconnect; load typed cached data.
                            this.status = status.lines().next().unwrap_or("Réception").into();
                            if status.starts_with("Connecté") {
                                this.refresh_timeline(cx);
                            }
                            cx.notify();
                        } else if poll_dm && matches!(this.target, Some(Target::Dm(_))) {
                            this.load(Some(Action::AutoSync), cx);
                        }
                    })
                    .is_ok();
                if !active {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_timeline(&mut self, cx: &mut Context<Self>) {
        if self.timeline_loading {
            return;
        }
        let Some(target) = self.target.clone() else {
            return;
        };
        let scope = self.scope.clone();
        let members = self.snapshot.members.clone();
        let limit = self.limit;
        self.timeline_loading = true;
        let revision = self.revision;
        let work_scope = scope.clone();
        let work_target = target.clone();
        let work = cx.background_executor().spawn(async move {
            service::timeline(&work_scope.0, &work_scope.1, &work_target, limit, &members)
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |entity, cx| {
            let messages = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.timeline_loading = false;
                if this.revision == revision
                    && this.scope == scope
                    && this.target.as_ref() == Some(&target)
                    && !this.busy
                {
                    match messages {
                        Ok(messages) => this.snapshot.messages = messages,
                        Err(error) => {
                            this.status = format!(
                                "Lecture locale impossible : {error}. Réessayez Synchroniser."
                            )
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn save_draft(&mut self, cx: &App) {
        if let Some(target) = &self.target {
            let body = self.composer.read(cx).content.to_string();
            self.drafts.save(&self.scope.0, &self.scope.1, target, body);
        }
    }

    fn restore_draft(&mut self, cx: &mut Context<Self>) {
        let body = self
            .drafts
            .get(&self.scope.0, &self.scope.1, self.target.as_ref());
        self.composer
            .update(cx, |input, cx| input.set_content(&body, cx));
    }

    fn context(&mut self, cx: &mut Context<Self>) {
        let origin = self.origin.read(cx).content.to_string();
        let conversation = match &self.target {
            Some(Target::Dm(id)) => Some(id.as_str()),
            _ => None,
        };
        self.settings.update(cx, |settings, cx| {
            settings.set_context(
                &origin,
                self.community.as_deref(),
                self.member.as_deref(),
                conversation,
                cx,
            )
        });
    }

    fn act(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.busy || (self.show_settings && self.settings.read(cx).is_busy()) {
            return;
        }
        self.operation_error = None;
        match action {
            Action::Settings => {
                self.save_draft(cx);
                if self.show_settings {
                    let connected = self.settings.update(cx, |settings, _| {
                        settings.suspend();
                        settings.take_connected_origin()
                    });
                    if let Some(origin) = connected {
                        self.origin
                            .update(cx, |input, cx| input.set_content(&origin, cx));
                    }
                    self.show_settings = false;
                    self.act(Action::Refresh, cx);
                } else {
                    self.context(cx);
                    self.settings.update(cx, |settings, cx| settings.resume(cx));
                    self.show_settings = true;
                }
                self.follow_selection();
            }
            Action::Community(id) => {
                self.save_draft(cx);
                self.community = Some(id);
                self.member = None;
                self.target = None;
                self.follow_selection();
                self.title = "Choisissez un salon".into();
                self.snapshot.messages.clear();
                self.restore_draft(cx);
                self.load(None, cx);
            }
            Action::Select(target, title) => {
                self.save_draft(cx);
                if let Target::Dm(id) = &target
                    && let Some(origin) = self.snapshot.conversation_sources.get(id).cloned()
                    && origin != self.scope.1
                {
                    self.scope.1 = origin.clone();
                    self.origin
                        .update(cx, |input, cx| input.set_content(&origin, cx));
                    self.community = None;
                    self.member = None;
                }
                self.target = Some(target);
                self.follow_selection();
                self.title = title;
                self.limit = 50;
                self.snapshot.messages.clear();
                self.restore_draft(cx);
                self.load(Some(Action::Sync), cx);
            }
            Action::Member(id, name) => {
                self.member = Some(id);
                self.status = format!(
                    "Membre sélectionné : {name}. Ouvrez Paramètres → Modération pour agir."
                );
                self.context(cx);
            }
            Action::Earlier => {
                self.limit = self.limit.saturating_add(50);
                self.load(None, cx);
            }
            Action::Refresh => {
                self.save_draft(cx);
                let scope = (
                    self.profile.read(cx).content.to_string(),
                    self.origin.read(cx).content.to_string(),
                );
                if scope != self.scope {
                    self.scope = scope;
                    self.community = None;
                    self.member = None;
                    self.target = None;
                    self.follow_selection();
                    self.snapshot = Snapshot::default();
                    self.title = "Choisissez une conversation".into();
                    self.restore_draft(cx);
                }
                self.load(None, cx);
            }
            operation => self.load(Some(operation), cx),
        }
        cx.notify();
    }

    fn load(&mut self, operation: Option<Action>, cx: &mut Context<Self>) {
        self.revision += 1;
        let profile = self.scope.0.clone();
        let origin = self.scope.1.clone();
        let community = self.community.clone();
        let target = self.target.clone();
        let body = self.composer.read(cx).content.to_string();
        let limit = self.limit;
        self.busy = true;
        self.status = "Chargement…".into();
        let send = matches!(operation, Some(Action::Send))
            || (matches!(operation, Some(Action::Retry | Action::AutoSync))
                && self
                    .snapshot
                    .messages
                    .iter()
                    .any(|message| message.pending && message.body == body));
        let work = cx.background_executor().spawn(async move {
            let operation_result = match (operation, target.as_ref()) {
                (Some(Action::Send), Some(target)) => {
                    service::send(&profile, &origin, target, &body)
                }
                (Some(Action::Sync), Some(target)) => service::sync(&profile, &origin, target),
                (Some(Action::AutoSync), Some(Target::Dm(id))) => crate::identity_service::execute(
                    &profile,
                    crate::identity_service::Command::DirectMessages {
                        origin: origin.clone(),
                        command: crate::dm_service::Command::AutoSync { id: id.clone() },
                    },
                )
                .map(|_| ()),
                (Some(Action::Retry), Some(Target::Channel(id))) => {
                    crate::identity_service::execute(
                        &profile,
                        crate::identity_service::Command::Chat {
                            origin: origin.clone(),
                            channel: id.clone(),
                            command: crate::chat_service::Command::Retry,
                        },
                    )
                    .map(|_| ())
                }
                (Some(Action::Retry), Some(Target::Dm(id))) => crate::identity_service::execute(
                    &profile,
                    crate::identity_service::Command::DirectMessages {
                        origin: origin.clone(),
                        command: crate::dm_service::Command::Retry { id: id.clone() },
                    },
                )
                .map(|_| ()),
                (Some(_), None) => Err("Sélectionnez un salon ou un MP.".into()),
                _ => Ok(()),
            }
            .map_err(|error| error.to_string());
            let snapshot = service::load(
                &profile,
                &origin,
                community.as_deref(),
                target.as_ref(),
                limit,
            )
            .map_err(|error| error.to_string());
            (operation_result, snapshot, body)
        });
        cx.spawn(async move |entity, cx| {
            let (result, snapshot, sent_body) = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                if result.is_ok() { this.operation_error = None; }
                if send && result.is_ok() && this.composer.read(cx).content.as_ref() == sent_body {
                    this.composer.update(cx, |input, cx| input.set_content("", cx));
                    this.save_draft(cx);
                }
                this.status = match snapshot {
                    Ok(snapshot) => {
                        let status = snapshot.status.clone();
                        this.snapshot = snapshot;
                        status
                    }
                    Err(error) => format!("Actualisation impossible : {error}. Vérifiez Connexion dans Paramètres puis Actualiser."),
                };
                if let Err(error) = result {
                    this.operation_error = Some(format!("{error} — Votre brouillon est conservé. Utilisez Réessayer pour un envoi en attente, ou Paramètres → Instance."));
                }
                cx.notify();
            });
        }).detach();
    }

    fn button(
        &mut self,
        id: String,
        label: String,
        selected: bool,
        action: Action,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focus = self
            .buttons
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let keyboard_action = action.clone();
        div()
            .id(SharedString::from(id))
            .track_focus(&focus)
            .tab_index(0)
            .px_3()
            .py_2()
            .rounded_md()
            .border_2()
            .border_color(rgb(0x263449))
            .focus(|style| style.border_color(rgb(0x93c5fd)))
            .bg(rgb(if selected { 0x1d4ed8 } else { 0x263449 }))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.act(action.clone(), cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if input::activates_button(&event.keystroke.key, event.keystroke.modifiers) {
                    cx.stop_propagation();
                    this.act(keyboard_action.clone(), cx);
                }
            }))
    }

    fn choices(
        &mut self,
        choices: Vec<Choice>,
        kind: &str,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        choices
            .into_iter()
            .map(|choice| {
                let (selected, action) = match kind {
                    "community" => (
                        self.community.as_ref() == Some(&choice.id),
                        Action::Community(choice.id.clone()),
                    ),
                    "member" => (
                        self.member.as_ref() == Some(&choice.id),
                        Action::Member(choice.id.clone(), choice.label.clone()),
                    ),
                    _ => {
                        let target = if kind == "channel" {
                            Target::Channel(choice.id.clone())
                        } else {
                            Target::Dm(choice.id.clone())
                        };
                        (
                            self.target.as_ref() == Some(&target),
                            Action::Select(target, choice.label.clone()),
                        )
                    }
                };
                self.button(
                    format!("{kind}-{}", choice.id),
                    choice.label,
                    selected,
                    action,
                    cx,
                )
                .into_any_element()
            })
            .collect()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh = self.button(
            "refresh".into(),
            "Actualiser".into(),
            false,
            Action::Refresh,
            cx,
        );
        let settings = self.button(
            "settings".into(),
            if self.show_settings {
                "Retour aux discussions"
            } else {
                "Paramètres / Bienvenue"
            }
            .into(),
            self.show_settings,
            Action::Settings,
            cx,
        );
        let header = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .p_3()
            .items_center()
            .child(div().text_xl().child("Kova"))
            .child(div().w(px(170.)).child(self.profile.clone()))
            .child(div().flex_1().min_w(px(180.)).child(self.origin.clone()))
            .child(refresh)
            .child(settings);
        let content = if self.show_settings {
            let community = self
                .snapshot
                .communities
                .iter()
                .find(|item| Some(&item.id) == self.community.as_ref())
                .map(|item| item.label.as_str())
                .unwrap_or("aucune");
            let member = self
                .snapshot
                .members
                .iter()
                .find(|item| Some(&item.id) == self.member.as_ref())
                .map(|item| item.label.as_str())
                .unwrap_or("aucun");
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(div().px_4().py_2().child(format!(
                    "Sélection : communauté {community} · membre {member} · {}",
                    self.title
                )))
                .child(self.settings.clone())
                .into_any_element()
        } else {
            let communities = self.choices(self.snapshot.communities.clone(), "community", cx);
            let channels = self.choices(self.snapshot.channels.clone(), "channel", cx);
            let conversations = self.choices(self.snapshot.conversations.clone(), "dm", cx);
            let members = self.choices(self.snapshot.members.clone(), "member", cx);
            let sync = self.button(
                "sync".into(),
                "Synchroniser".into(),
                false,
                Action::Sync,
                cx,
            );
            let earlier = self.button(
                "earlier".into(),
                "Afficher plus tôt".into(),
                false,
                Action::Earlier,
                cx,
            );
            let send = self.button(
                "send".into(),
                if self.busy { "Patientez…" } else { "Envoyer" }.into(),
                false,
                Action::Send,
                cx,
            );
            let retry = self.button(
                "retry".into(),
                "Réessayer l’envoi en attente".into(),
                false,
                Action::Retry,
                cx,
            );
            div().flex().flex_1().min_h_0().gap_2().px_3()
                .child(div().id("communities").w(px(180.)).flex_shrink_0().overflow_y_scroll().flex().flex_col().gap_2()
                    .child(div().text_lg().child("Communautés")).children(communities)
                    .when(self.snapshot.communities.is_empty(), |el| el.child("Connectez-vous depuis Paramètres, puis actualisez.")))
                .child(div().id("navigation").w(px(235.)).flex_shrink_0().overflow_y_scroll().flex().flex_col().gap_2()
                    .child(div().text_lg().child("Salons")).children(channels)
                    .child(div().text_lg().mt_3().child("Messages privés")).children(conversations)
                    .child(div().text_sm().child("Une proposition reçue doit être vérifiée puis acceptée dans Paramètres → MP."))
                    .child(div().text_lg().mt_3().child("Membres")).children(members))
                .child(div().flex_1().min_w_0().flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().gap_2().items_center().child(div().text_lg().flex_1().child(self.title.clone())).child(sync))
                    .child(div().id("messages").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_3().p_3().bg(rgb(0x182335))
                        .child(earlier)
                        .children(self.snapshot.messages.iter().map(|message| div().flex().flex_col().gap_1().p_3().rounded_md().bg(rgb(0x263449))
                            .child(div().text_sm().text_color(rgb(0xb8c8df)).child(format!("{} · {} · {}", message.author, message.date, if message.pending { "En attente" } else { "Reçu par le serveur" })))
                            .child(message.body.clone())))
                        .when(self.target.is_none(), |el| el.child("Choisissez un salon ou un MP dans la liste. Pour commencer, ouvrez Paramètres / Bienvenue.")))
                    .child(self.composer.clone())
                    .child(div().flex().flex_wrap().gap_2().child(send).child(retry))
                    .child(div().text_sm().child("Brouillons conservés pendant la navigation · Tab / Maj+Tab : naviguer · Entrée : activer")))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x101827))
            .text_color(rgb(0xe5edf9))
            .text_base()
            .on_key_down(|event, window, cx| {
                let modifiers = Modifiers {
                    shift: false,
                    ..event.keystroke.modifiers
                };
                if event.keystroke.key == "tab" && !modifiers.modified() {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                }
            })
            .child(header)
            .child(content)
            .child(
                div()
                    .id("connection-status")
                    .h(px(100.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .p_3()
                    .text_sm()
                    .when_some(self.operation_error.clone(), |element, error| {
                        element.child(div().text_color(rgb(0xfca5a5)).child(error))
                    })
                    .child(div().child(self.status.clone())),
            )
    }
}

pub fn run() {
    Application::new().run(|cx| {
        input::bind_keys(cx);
        cx.open_window(WindowOptions {
            window_min_size: Some(size(px(900.), px(600.))),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1220.), px(820.)), cx))),
            titlebar: Some(TitlebarOptions { title: Some("Kova".into()), ..Default::default() }),
            ..Default::default()
        }, |window, cx| cx.new(|cx| {
            let profile_name = std::env::var("KOVA_UI_PROFILE").unwrap_or_else(|_| "default".into());
            let origin_name = std::env::var("KOVA_UI_ORIGIN").unwrap_or_default();
            let profile = cx.new(|cx| TextInput::new(cx, "Profil local", &profile_name, false));
            window.focus(&profile.focus_handle(cx));
            let origin = cx.new(|cx| TextInput::new(cx, "Instance https://…", &origin_name, false));
            let settings = cx.new(|cx| IdentityWindow::new(profile.clone(), cx));
            let workspace = Workspace {
                profile, origin, settings, show_settings: false,
                composer: cx.new(|cx| TextInput::new(cx, "Votre message…", "", false)),
                snapshot: Snapshot::default(), community: None, member: None, target: None,
                title: "Bienvenue sur Kova".into(), scope: (profile_name, origin_name),
                drafts: Drafts::default(), buttons: BTreeMap::new(), limit: 50, busy: false,
                status: "Créez ou restaurez votre identité dans Paramètres / Bienvenue, puis connectez-vous à une instance.".into(),
                subscription: None,
                timeline_loading: false,
                operation_error: None,
                revision: 0,
            };
            workspace.start_updates(cx);
            workspace
        })).expect("open Kova window");
        cx.activate(true);
        cx.on_window_closed(|cx| { if cx.windows().is_empty() { cx.quit(); } }).detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{Drafts, Target};

    #[test]
    fn navigating_between_accounts_instances_and_threads_preserves_distinct_drafts() {
        let mut drafts = Drafts::default();
        let channel = Target::Channel("same-id".into());
        let dm = Target::Dm("same-id".into());
        drafts.save(
            "alice",
            "a",
            &channel,
            "Public à conserver après échec".into(),
        );
        drafts.save("alice", "a", &dm, "Privé".into());
        drafts.save("bob", "a", &channel, "Autre profil".into());
        drafts.save("alice", "b", &channel, "Autre instance".into());
        assert_eq!(
            drafts.get("alice", "a", Some(&channel)),
            "Public à conserver après échec"
        );
        assert_eq!(drafts.get("alice", "a", Some(&dm)), "Privé");
        assert_eq!(drafts.get("bob", "a", Some(&channel)), "Autre profil");
        assert_eq!(drafts.get("alice", "b", Some(&channel)), "Autre instance");
        drafts.save("alice", "a", &channel, String::new());
        assert!(drafts.get("alice", "a", Some(&channel)).is_empty());
        assert_eq!(drafts.get("alice", "a", Some(&dm)), "Privé");
    }
}
