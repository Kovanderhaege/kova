use crate::{
    identity_service::{self, Command},
    input::{self, TextInput},
};
use gpui::{prelude::*, *};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
    Chat,
    Communities,
    Moderation,
    DirectMessages,
    Identity,
    Connection,
    Backup,
    Request,
    Approval,
    Association,
    Revocation,
}
impl Page {
    fn fields(self) -> &'static [(&'static str, bool)] {
        match self {
            Self::Chat => &[
                ("Origine exacte de l’instance connectée", false),
                ("Identifiant du salon (copier depuis Salons)", false),
                ("Message public (1–4000 caractères)", false),
            ],
            Self::Communities => &[
                ("Origine exacte de l'instance connectée", false),
                ("Identifiant de communauté (copier depuis la liste)", false),
                ("Nom de la nouvelle communauté ou du nouveau salon", false),
            ],
            Self::Moderation => &[
                ("Origine exacte de l'instance connectée", false),
                ("Identifiant de communauté", false),
                (
                    "Identifiant du membre (copier depuis la liste des membres)",
                    false,
                ),
            ],
            Self::DirectMessages => &[
                ("Origine exacte de l'instance connectée", false),
                (
                    "Identité globale du correspondant (vérifiée par canal sûr)",
                    false,
                ),
                (
                    "Empreinte complète de son appareil vérifiée hors bande",
                    false,
                ),
                ("Identifiant global de conversation", false),
                ("Message privé", false),
                ("Origine du réplica à proposer / synchroniser", false),
            ],
            Self::Identity => &[],
            Self::Connection => &[
                (
                    "Origine exacte (https://… ; HTTP permis sur localhost)",
                    false,
                ),
                ("Invitation (vide pour se reconnecter)", true),
                ("Identifiant local", false),
                ("Nom affiché", false),
            ],
            Self::Backup => &[
                ("Chemin absolu du fichier .age", false),
                ("Phrase secrète (12 caractères minimum pour créer)", true),
            ],
            Self::Request => &[("Nouveau fichier de demande .json", false)],
            Self::Approval => &[
                ("Fichier de demande .json", false),
                ("Empreinte complète vérifiée sur l'autre appareil", false),
                ("Nouveau fichier de réponse .json", false),
            ],
            Self::Association => &[
                ("Fichier de réponse .json", false),
                (
                    "Clé publique racine vérifiée sur l'appareil de confiance",
                    false,
                ),
            ],
            Self::Revocation => &[(
                "Clé publique de l'appareil à révoquer définitivement",
                false,
            )],
        }
    }
    fn actions(self) -> &'static [&'static str] {
        match self {
            Self::Chat => &[
                "Recevoir en direct",
                "Arrêter le direct",
                "Synchroniser",
                "Envoyer",
                "Réessayer l’envoi",
                "Hors ligne",
            ],
            Self::Communities => &[
                "Mes communautés",
                "Créer la communauté",
                "Salons",
                "Créer le salon",
                "Membres",
                "Inviter (24 h)",
            ],
            Self::Moderation => &[
                "Nommer modérateur",
                "Rétablir membre",
                "Exclure définitivement",
            ],
            Self::DirectMessages => &[
                "Publier mon appareil",
                "Lister les conversations de l'instance",
                "Créer une conversation",
                "Accepter la conversation sélectionnée",
                "Synchroniser",
                "Envoyer",
                "Réessayer l'envoi en attente",
                "Hors ligne (cache local)",
                "Préparer une proposition de réplica",
                "Consulter la proposition",
                "Approuver la proposition consultée",
                "Synchroniser avec le réplica approuvé",
                "Proposer le retrait d'un réplica",
                "Renouveler les preuves de cet appareil",
            ],
            Self::Identity => &["Afficher l'identité", "Créer une identité"],
            Self::Connection => &[
                "Se connecter / rejoindre",
                "Lister les appareils",
                "Se déconnecter",
            ],
            Self::Backup => &["Créer la sauvegarde", "Restaurer dans ce profil vide"],
            Self::Request => &["Créer / renouveler la demande"],
            Self::Approval => &["Confirmer l'empreinte et autoriser"],
            Self::Association => &["Vérifier la racine et associer"],
            Self::Revocation => &["Révoquer définitivement", "Réessayer la transmission"],
        }
    }
    fn guidance(self) -> &'static str {
        match self {
            Self::Chat => {
                "Les messages sont visibles par les membres du salon. Le direct reprend après une coupure. En cas d’envoi incertain, utilisez Réessayer ; le texte en attente est conservé localement."
            }
            Self::Communities => {
                "Connectez-vous d'abord à l'instance. Créer une communauté crée aussi son salon général. Pour rejoindre avec une invitation, utilisez Connexion. Copiez un identifiant de salon dans le panneau Chat."
            }
            Self::Moderation => {
                "Seul le propriétaire change les rôles. Un modérateur peut exclure un membre simple. L'exclusion est définitive pour cette communauté, même avec une nouvelle invitation. Vérifiez l'identifiant avant de cliquer."
            }
            Self::DirectMessages => {
                "Vérifiez l'identité et l'empreinte de l'appareil par un canal indépendant avant toute création ou acceptation. La conversation sélectionnée est synchronisée automatiquement toutes les 30 secondes et les envois en attente sont réessayés sur une instance approuvée disponible. Hors ligne ou pendant la préparation, l'examen et l'approbation d'une politique, le travail de fond est suspendu ; utilisez Synchroniser, Envoyer ou Réessayer pour le reprendre. Pour la réplication, préparez une politique, consultez les origines et clés, puis approuvez séparément. Les deux participants doivent approuver avant toute synchronisation. Les autorisations d'appareil expirent après 24 h : utilisez « Renouveler les preuves de cet appareil » pour prolonger la conversation en conservant son historique et sa file d'envoi. Chaque participant renouvelle son propre appareil ; si cet appareil est associé, faites renouveler son autorisation depuis l'appareil racine puis réassociez-le. Reconnectez-vous à un réplica si sa session a expiré. Une révocation peut n'être connue d'un réplica isolé qu'après son retour en ligne."
            }
            Self::Identity => {
                "La clé racine est protégée par Windows. Créez une sauvegarde : sa perte sans sauvegarde rend l'identité irrécupérable."
            }
            Self::Connection => {
                "Le profil local et la session sont propres à chaque instance. Pour une première adhésion, indiquez une invitation et un nom local."
            }
            Self::Backup => {
                "Conservez la phrase secrète séparément. La restauration crée un nouvel appareil avec la même identité ; elle ne révoque pas les anciens appareils. Aucun fichier existant n'est écrasé."
            }
            Self::Request => {
                "Sur le nouvel appareil, exportez une demande et transmettez son empreinte complète par un canal de confiance. La demande expire après 10 minutes."
            }
            Self::Approval => {
                "Sur l'appareil racine, comparez l'empreinte au nouvel appareil avant de confirmer. L'autorisation expire après 24 heures au maximum."
            }
            Self::Association => {
                "Sur le nouvel appareil, importez la réponse. Vérifiez la clé publique racine affichée sur l'appareil de confiance avant de confirmer."
            }
            Self::Revocation => {
                "Action définitive signée par la racine. Copiez la clé depuis la liste des appareils. Les instances inaccessibles peuvent accepter l'appareil jusqu'à expiration de son autorisation (24 h maximum)."
            }
        }
    }
}
pub(crate) struct IdentityWindow {
    profile: Entity<TextInput>,
    page: Page,
    fields: Vec<Entity<TextInput>>,
    status: String,
    operation_status: String,
    subscription_scope: Option<(String, String, String)>,
    busy: bool,
    dm_background_paused: bool,
    subscription: Option<crate::chat_service::Subscription>,
    subscription_generation: u64,
    drafts: std::collections::HashMap<Page, Vec<String>>,
    workspace_origin: String,
    workspace_community: Option<String>,
    workspace_member: Option<String>,
    workspace_conversation: Option<String>,
    focus_handles: std::collections::HashMap<String, FocusHandle>,
    connected_origin: Option<String>,
    diagnostics: bool,
}

#[derive(Clone)]
enum SettingsAction {
    Navigate(Page),
    Execute(usize),
    CopyResult,
}

impl IdentityWindow {
    pub(crate) fn new(profile: Entity<TextInput>, cx: &mut Context<Self>) -> Self {
        let page = Page::Identity;
        let fields = page
            .fields()
            .iter()
            .map(|(label, secret)| cx.new(|cx| TextInput::new(cx, label, "", *secret)))
            .collect();
        Self {
            profile,
            page,
            fields,
            status: "Commencez par créer une identité ou vous connecter.".into(),
            operation_status: String::new(),
            subscription_scope: None,
            busy: false,
            dm_background_paused: false,
            subscription: None,
            subscription_generation: 0,
            drafts: std::collections::HashMap::new(),
            workspace_origin: String::new(),
            workspace_community: None,
            workspace_member: None,
            workspace_conversation: None,
            focus_handles: std::collections::HashMap::new(),
            connected_origin: None,
            diagnostics: false,
        }
    }

    pub(crate) fn suspend(&mut self) {
        self.subscription = None;
        self.subscription_generation += 1;
        self.dm_background_paused = true;
    }

    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::DirectMessages && self.dm_background_paused {
            self.dm_background_paused = false;
            self.start_dm_background_sync(cx);
        }
    }

    pub(crate) fn take_connected_origin(&mut self) -> Option<String> {
        self.connected_origin.take()
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn set_context(
        &mut self,
        origin: &str,
        community: Option<&str>,
        member: Option<&str>,
        conversation: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.remember_fields(cx);
        self.workspace_origin = origin.to_owned();
        self.workspace_community = community.map(str::to_owned);
        self.workspace_member = member.map(str::to_owned);
        self.workspace_conversation = conversation.map(str::to_owned);
        for page in [
            Page::Chat,
            Page::Communities,
            Page::Moderation,
            Page::DirectMessages,
            Page::Connection,
        ] {
            let mut values = self.drafts.get(&page).cloned().unwrap_or_default();
            values.resize(page.fields().len(), String::new());
            if page != Page::Connection || !origin.is_empty() {
                values[0] = origin.to_owned();
            }
            match page {
                Page::Communities => {
                    values[1] = self.workspace_community.clone().unwrap_or_default();
                }
                Page::Moderation => {
                    values[1] = self.workspace_community.clone().unwrap_or_default();
                    values[2] = self.workspace_member.clone().unwrap_or_default();
                }
                Page::DirectMessages => {
                    values[3] = self.workspace_conversation.clone().unwrap_or_default();
                }
                _ => {}
            }
            self.drafts.insert(page, values.clone());
            if self.page == page {
                for (index, field) in self.fields.iter().enumerate() {
                    field.update(cx, |input, cx| {
                        input.set_content(&values[index], cx);
                    });
                }
            }
        }
        cx.notify();
    }

    fn remember_fields(&mut self, cx: &Context<Self>) {
        let values = self
            .fields
            .iter()
            .map(|field| field.read(cx).content.to_string())
            .collect();
        self.drafts
            .insert(self.page, non_secret_draft(self.page, values));
    }

    fn focusable_action(
        &mut self,
        id: String,
        label: String,
        selected: bool,
        action: SettingsAction,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focus = self
            .focus_handles
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let keyboard_action = action.clone();
        div()
            .id(SharedString::from(id))
            .track_focus(&focus)
            .tab_index(0)
            .p_2()
            .border_2()
            .border_color(rgb(0x475569))
            .focus(|style| style.border_color(rgb(0xfbbf24)))
            .cursor_pointer()
            .bg(rgb(if selected { 0x2563eb } else { 0x334155 }))
            .child(label)
            .on_click(
                cx.listener(move |this, _, _, cx| this.perform_settings_action(action.clone(), cx)),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if input::activates_button(&event.keystroke.key, event.keystroke.modifiers) {
                    cx.stop_propagation();
                    this.perform_settings_action(keyboard_action.clone(), cx);
                }
            }))
    }

    fn perform_settings_action(&mut self, action: SettingsAction, cx: &mut Context<Self>) {
        match action {
            SettingsAction::Navigate(page) => self.navigate(page, cx),
            SettingsAction::Execute(index) => self.execute(index, cx),
            SettingsAction::CopyResult => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.status.clone()));
            }
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.remember_fields(cx);
        self.subscription = None;
        self.subscription_generation += 1;
        self.page = page;
        let mut values = self.drafts.get(&page).cloned().unwrap_or_default();
        values.resize(page.fields().len(), String::new());
        if let Some(origin_index) = matches!(
            page,
            Page::Chat
                | Page::Communities
                | Page::Moderation
                | Page::DirectMessages
                | Page::Connection
        )
        .then_some(0)
            && values[origin_index].is_empty()
        {
            values[origin_index] = self.workspace_origin.clone();
        }
        match page {
            Page::Communities => {
                values[1] = self.workspace_community.clone().unwrap_or_default();
            }
            Page::Moderation => {
                values[1] = self.workspace_community.clone().unwrap_or_default();
                values[2] = self.workspace_member.clone().unwrap_or_default();
            }
            Page::DirectMessages => {
                if let Some(conversation) = &self.workspace_conversation {
                    values[3] = conversation.clone();
                }
            }
            _ => {}
        }
        self.fields = page
            .fields()
            .iter()
            .enumerate()
            .map(|(index, (label, secret))| {
                cx.new(|cx| TextInput::new(cx, label, &values[index], *secret))
            })
            .collect();
        self.status.clear();
        self.operation_status.clear();
        self.dm_background_paused = false;
        if page == Page::DirectMessages {
            self.start_dm_background_sync(cx);
        }
        cx.notify();
    }
    fn start_dm_background_sync(&self, cx: &mut Context<Self>) {
        let generation = self.subscription_generation;
        cx.spawn(async move |entity, cx| {
            let mut delay = std::time::Duration::from_secs(30);
            loop {
                Timer::after(delay).await;
                let request = entity
                    .update(cx, |this, cx| {
                        if this.subscription_generation != generation
                            || this.page != Page::DirectMessages
                        {
                            return None;
                        }
                        if this.busy || this.dm_background_paused {
                            return Some(None);
                        }
                        let origin = this.fields[0].read(cx).content.to_string();
                        let id = this.fields[3].read(cx).content.to_string();
                        if origin.is_empty() || id.is_empty() {
                            return Some(None);
                        }
                        this.busy = true;
                        cx.notify();
                        Some(Some((
                            this.profile.read(cx).content.to_string(),
                            origin,
                            id,
                        )))
                    })
                    .unwrap_or(None);
                let (profile, origin, id) = match request {
                    None => break,
                    Some(None) => continue,
                    Some(Some(request)) => request,
                };
                let expected_profile = profile.clone();
                let expected_origin = origin.clone();
                let expected_id = id.clone();
                let work = cx.background_executor().spawn(async move {
                    let vault = kova_client_core::vault::Vault::new(&profile)
                        .map_err(|error| format!("Échec : {error}"))?;
                    crate::dm_service::execute(
                        &vault,
                        &origin,
                        crate::dm_service::Command::AutoSync { id },
                    )
                    .map_err(|error| format!("Échec : {error}"))
                });
                let result = work.await;
                let succeeded = result.is_ok();
                let keep_running = entity
                    .update(cx, |this, cx| {
                        this.busy = false;
                        if this.subscription_generation != generation
                            || this.page != Page::DirectMessages
                        {
                            return false;
                        }
                        if this.profile.read(cx).content.as_ref() != expected_profile
                            || this.fields[0].read(cx).content.as_ref() != expected_origin
                            || this.fields[3].read(cx).content.as_ref() != expected_id
                            || this.dm_background_paused
                        {
                            return true;
                        }
                        match result {
                            Ok(status) => {
                                this.operation_status =
                                    "Synchronisation automatique terminée.".into();
                                this.status = status;
                            }
                            Err(error) => {
                                this.operation_status =
                                    format!("Synchronisation automatique en attente : {error}");
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
                delay = if succeeded {
                    std::time::Duration::from_secs(30)
                } else {
                    (delay * 2).min(std::time::Duration::from_secs(300))
                };
            }
        })
        .detach();
    }
    fn execute(&mut self, action: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.page == Page::DirectMessages
            && let Some(paused) = dm_background_pause_after_action(action)
        {
            self.dm_background_paused = paused;
        }
        if self.page == Page::Chat && action <= 1 {
            self.subscription = None;
            self.subscription_generation += 1;
            if action == 1 {
                self.status = "Réception arrêtée.".into();
                cx.notify();
                return;
            }
            let subscription = crate::chat_service::Subscription::start(
                self.profile.read(cx).content.to_string(),
                self.fields[0].read(cx).content.to_string(),
                self.fields[1].read(cx).content.to_string(),
            );
            let latest = subscription.latest.clone();
            let generation = self.subscription_generation;
            self.subscription_scope = Some((
                self.profile.read(cx).content.to_string(),
                self.fields[0].read(cx).content.to_string(),
                self.fields[1].read(cx).content.to_string(),
            ));
            self.subscription = Some(subscription);
            self.status = "Connexion au salon…".into();
            cx.spawn(async move |entity, cx| {
                loop {
                    Timer::after(std::time::Duration::from_millis(100)).await;
                    let active = entity
                        .update(cx, |this, cx| {
                            if this.subscription_generation != generation {
                                return false;
                            }
                            if let Ok(mut slot) = latest.lock()
                                && let Some(status) = slot.take()
                            {
                                this.status = status;
                                cx.notify();
                            }
                            true
                        })
                        .unwrap_or(false);
                    if !active {
                        break;
                    }
                }
            })
            .detach();
            cx.notify();
            return;
        }
        let keep_stream = self.page == Page::Chat
            && matches!(action, 3 | 4)
            && self.subscription_scope.as_ref()
                == Some(&(
                    self.profile.read(cx).content.to_string(),
                    self.fields[0].read(cx).content.to_string(),
                    self.fields[1].read(cx).content.to_string(),
                ));
        if !keep_stream {
            self.subscription = None;
            self.subscription_generation += 1;
        }
        let value = |index: usize| self.fields[index].read(cx).content.to_string();
        let command = match (self.page, action) {
            (Page::Chat, action) => Command::Chat {
                origin: value(0),
                channel: value(1),
                command: match action {
                    2 => crate::chat_service::Command::Sync,
                    3 => crate::chat_service::Command::Send(value(2)),
                    4 => crate::chat_service::Command::Retry,
                    _ => crate::chat_service::Command::Offline,
                },
            },
            (Page::Communities, action) => {
                use crate::community_service::Command as Community;
                Command::Community {
                    origin: value(0),
                    command: match action {
                        0 => Community::List,
                        1 => Community::Create { name: value(2) },
                        2 => Community::Channels {
                            community: value(1),
                        },
                        3 => Community::CreateChannel {
                            community: value(1),
                            name: value(2),
                        },
                        4 => Community::Members {
                            community: value(1),
                        },
                        _ => Community::Invite {
                            community: value(1),
                        },
                    },
                }
            }
            (Page::Moderation, action) => {
                use crate::community_service::Command as Community;
                Command::Community {
                    origin: value(0),
                    command: match action {
                        0 => Community::SetRole {
                            community: value(1),
                            member: value(2),
                            role: "moderator",
                        },
                        1 => Community::SetRole {
                            community: value(1),
                            member: value(2),
                            role: "member",
                        },
                        _ => Community::Exclude {
                            community: value(1),
                            member: value(2),
                        },
                    },
                }
            }
            (Page::DirectMessages, action) => Command::DirectMessages {
                origin: value(0),
                command: match action {
                    0 => crate::dm_service::Command::Publish,
                    1 => crate::dm_service::Command::List,
                    2 => crate::dm_service::Command::Create {
                        peer_identity: value(1),
                        peer_device: value(2),
                    },
                    3 => crate::dm_service::Command::Accept {
                        id: value(3),
                        peer_identity: value(1),
                        peer_device: value(2),
                    },
                    4 => crate::dm_service::Command::Sync { id: value(3) },
                    5 => crate::dm_service::Command::Send {
                        id: value(3),
                        body: value(4),
                    },
                    6 => crate::dm_service::Command::Retry { id: value(3) },
                    7 => crate::dm_service::Command::Offline { id: value(3) },
                    8 => crate::dm_service::Command::ProposeReplica {
                        id: value(3),
                        peer_origin: value(5),
                    },
                    9 => crate::dm_service::Command::ReviewReplica { id: value(3) },
                    10 => crate::dm_service::Command::ApproveReplica { id: value(3) },
                    11 => crate::dm_service::Command::SyncReplica {
                        id: value(3),
                        peer_origin: value(5),
                    },
                    12 => crate::dm_service::Command::RemoveReplica {
                        id: value(3),
                        peer_origin: value(5),
                    },
                    _ => crate::dm_service::Command::RenewAuthorization { id: value(3) },
                },
            },
            (Page::Identity, 0) => Command::Inspect,
            (Page::Identity, _) => Command::Create,
            (Page::Connection, 0) => Command::Connect {
                origin: value(0),
                invite: value(1),
                handle: value(2),
                display: value(3),
            },
            (Page::Connection, 1) => Command::Devices { origin: value(0) },
            (Page::Connection, _) => Command::Logout { origin: value(0) },
            (Page::Backup, 0) => Command::Backup {
                path: value(0),
                password: Zeroizing::new(value(1)),
            },
            (Page::Backup, _) => Command::Restore {
                path: value(0),
                password: Zeroizing::new(value(1)),
            },
            (Page::Request, _) => Command::Request { path: value(0) },
            (Page::Approval, _) => Command::Approve {
                path: value(0),
                fingerprint: value(1),
                output: value(2),
            },
            (Page::Association, _) => Command::Complete {
                path: value(0),
                root: value(1),
            },
            (Page::Revocation, 0) => Command::Revoke {
                public_key: value(0),
            },
            (Page::Revocation, _) => Command::Retry,
        };
        let profile = self.profile.read(cx).content.to_string();
        let connection_origin = match &command {
            Command::Connect { origin, .. } => Some(origin.clone()),
            _ => None,
        };
        for (field, (_, secret)) in self.fields.iter().zip(self.page.fields()) {
            if *secret {
                field.update(cx, |input, cx| {
                    input.reset();
                    cx.notify();
                });
            }
        }
        self.busy = true;
        self.operation_status = "Opération en cours…".into();
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            identity_service::execute(&profile, command).map_err(|error| format!("Échec : {error}"))
        });
        cx.spawn(async move |entity, cx| {
            let status = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                let success = status.is_ok();
                let status = status.unwrap_or_else(|error| error);
                if success && let Some(origin) = connection_origin {
                    this.connected_origin = Some(origin);
                }
                if this.page == Page::Chat && matches!(action, 3 | 4) && success {
                    this.fields[2].update(cx, |input, cx| {
                        input.reset();
                        cx.notify();
                    });
                }
                if this.page == Page::DirectMessages && matches!(action, 5 | 6) && success {
                    this.fields[4].update(cx, |input, cx| {
                        input.reset();
                        cx.notify();
                    });
                }
                this.operation_status = if success {
                    "Opération terminée.".into()
                } else {
                    status.clone()
                };
                if this.subscription.is_none() {
                    this.status = status;
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn non_secret_draft(page: Page, values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .zip(page.fields())
        .map(|(value, (_, secret))| if *secret { String::new() } else { value })
        .collect()
}

fn dm_background_pause_after_action(action: usize) -> Option<bool> {
    match action {
        4..=6 | 13 => Some(false),
        7..=10 | 12 => Some(true),
        _ => None,
    }
}

impl Render for IdentityWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let diagnostics = self.diagnostics;
        let pages = [
            (Page::Identity, "Identité"),
            (Page::Connection, "Instance"),
            (Page::Communities, "Communautés et salons"),
            (Page::Chat, "Chat public"),
            (Page::DirectMessages, "Messages privés"),
            (Page::Moderation, "Membres et rôles"),
            (Page::Backup, "Sauvegarde et restauration"),
            (Page::Request, "Nouvel appareil"),
            (Page::Approval, "Autoriser un appareil"),
            (Page::Association, "Associer cet appareil"),
            (Page::Revocation, "Révoquer un appareil"),
        ];
        let hidden_fields: &[usize] = match (self.diagnostics, self.page) {
            (false, Page::Chat | Page::Communities) => &[1],
            (false, Page::Moderation) => &[1, 2],
            (false, Page::DirectMessages) => &[3],
            _ => &[],
        };
        div()
            .id("identity-scroll")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x111827))
            .child(
                div()
                    .min_h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .text_color(rgb(0xe5e7eb))
                    .when(diagnostics, |element| {
                        element.child("Profil local").child(self.profile.clone())
                    })
                    .child(div().text_xl().child(match self.page {
                        Page::Identity => "Votre identité",
                        Page::Connection => "Connexion à une instance",
                        Page::Communities => "Communautés et salons",
                        Page::Chat => "Chat public",
                        Page::DirectMessages => "Messages privés",
                        Page::Moderation => "Membres et rôles",
                        Page::Backup => "Sauvegarder ou restaurer",
                        Page::Request => "Associer un nouvel appareil",
                        Page::Approval => "Autoriser un appareil",
                        Page::Association => "Terminer l’association",
                        Page::Revocation => "Révoquer un appareil",
                    }))
                    .child(
                        div().flex().flex_wrap().gap_2().children(
                            pages
                                .into_iter()
                                .enumerate()
                                .filter(|(_, (page, _))| diagnostics || *page != Page::Chat)
                                .map(|(index, (page, label))| {
                                    self.focusable_action(
                                        format!("settings-page-{index}"),
                                        label.to_owned(),
                                        self.page == page,
                                        SettingsAction::Navigate(page),
                                        cx,
                                    )
                                }),
                        ),
                    )
                    .child(self.page.guidance())
                    .children(
                        self.fields
                            .iter()
                            .zip(self.page.fields())
                            .enumerate()
                            .filter(|(index, _)| !hidden_fields.contains(index))
                            .map(|(_, (field, (label, _)))| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(*label)
                                    .child(field.clone())
                            }),
                    )
                    .child(
                        div().flex().flex_wrap().gap_2().children(
                            self.page
                                .actions()
                                .iter()
                                .enumerate()
                                .map(|(index, label)| {
                                    self.focusable_action(
                                        format!("settings-action-{index}"),
                                        (*label).to_owned(),
                                        false,
                                        SettingsAction::Execute(index),
                                        cx,
                                    )
                                }),
                        ),
                    )
                    .child(self.operation_status.clone())
                    .child(
                        div()
                            .id("result")
                            .min_h(px(200.))
                            .flex_1()
                            .overflow_y_scroll()
                            .p_2()
                            .bg(rgb(0x1f2937))
                            .child(self.status.clone()),
                    )
                    .child(self.focusable_action(
                        "settings-copy-result".into(),
                        "Copier le résultat (une invitation est confidentielle)".into(),
                        false,
                        SettingsAction::CopyResult,
                        cx,
                    )),
            )
    }
}
pub fn run() {
    Application::new().run(|cx| {
        input::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1050.), px(850.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kova — Identité".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                let profile = cx.new(|cx| TextInput::new(cx, "Profil local", "default", false));
                cx.new(|cx| {
                    let mut view = IdentityWindow::new(profile, cx);
                    view.diagnostics = true;
                    view
                })
            },
        )
        .expect("open identity window");
        cx.activate(true);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{Page, dm_background_pause_after_action, non_secret_draft};

    #[test]
    fn page_drafts_preserve_regular_fields_and_clear_secrets() {
        let values = vec![
            "https://example.test".into(),
            "invitation-token".into(),
            "alice".into(),
            "Alice".into(),
        ];
        let draft = non_secret_draft(Page::Connection, values);

        assert_eq!(draft, ["https://example.test", "", "alice", "Alice"]);
    }

    #[test]
    fn offline_and_replica_review_pause_background_until_manual_sync() {
        let mut paused = false;
        for action in [7, 8, 9, 10, 12] {
            paused = dm_background_pause_after_action(action).unwrap_or(paused);
            assert!(paused, "action {action} should pause automatic sync");
        }
        for action in [4, 5, 6, 13] {
            paused = dm_background_pause_after_action(action).unwrap_or(paused);
            assert!(!paused, "action {action} should resume automatic sync");
        }
    }
}
