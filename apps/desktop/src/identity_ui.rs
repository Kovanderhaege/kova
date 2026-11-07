use crate::{
    identity_service::{self, Command},
    input::{self, TextInput},
};
use gpui::{prelude::*, *};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq)]
enum Page {
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
struct IdentityWindow {
    profile: Entity<TextInput>,
    page: Page,
    fields: Vec<Entity<TextInput>>,
    status: String,
    busy: bool,
}
impl IdentityWindow {
    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.page = page;
        self.fields = page
            .fields()
            .iter()
            .map(|(label, secret)| cx.new(|cx| TextInput::new(cx, label, "", *secret)))
            .collect();
        self.status.clear();
        cx.notify();
    }
    fn execute(&mut self, action: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let value = |index: usize| self.fields[index].read(cx).content.to_string();
        let command = match (self.page, action) {
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
        for (field, (_, secret)) in self.fields.iter().zip(self.page.fields()) {
            if *secret {
                field.update(cx, |input, cx| {
                    input.reset();
                    cx.notify();
                });
            }
        }
        self.busy = true;
        self.status = "Opération en cours…".into();
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            identity_service::execute(&profile, command)
                .unwrap_or_else(|error| format!("Échec : {error}"))
        });
        cx.spawn(async move |entity, cx| {
            let status = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                this.status = status;
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for IdentityWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = [
            (Page::Identity, "Identité"),
            (Page::Connection, "Connexion"),
            (Page::Backup, "Sauvegarde"),
            (Page::Request, "Demande"),
            (Page::Approval, "Autoriser"),
            (Page::Association, "Associer"),
            (Page::Revocation, "Révocation"),
        ];
        div().size_full().flex().flex_col().gap_3().p_4().bg(rgb(0x111827)).text_color(rgb(0xe5e7eb))
            .child(div().text_xl().child("Kova — Identité"))
            .child("Profil local (default pour votre profil habituel ; nouveau nom pour restaurer ou associer)")
            .child(self.profile.clone())
            .child(div().flex().gap_2().children(pages.into_iter().enumerate().map(|(index,(page,label))| {
                div().id(("page",index)).p_2().cursor_pointer().bg(rgb(if self.page==page {0x2563eb} else {0x334155})).child(label)
                    .on_click(cx.listener(move |this,_,_,cx| this.navigate(page,cx)))
            })))
            .child(self.page.guidance())
            .children(self.fields.iter().zip(self.page.fields()).map(|(field,(label,_))| div().flex().flex_col().gap_1().child(*label).child(field.clone())))
            .child(div().flex().gap_2().children(self.page.actions().iter().enumerate().map(|(index,label)| {
                div().id(("action",index)).p_2().cursor_pointer().bg(rgb(if self.busy {0x475569} else {0x1d4ed8})).child(*label)
                    .on_click(cx.listener(move |this,_,_,cx| this.execute(index,cx)))
            })))
            .child(div().id("result").flex_1().overflow_y_scroll().p_2().bg(rgb(0x1f2937)).child(self.status.clone()))
            .child(div().id("copy-result").cursor_pointer().child("Copier le résultat public")
                .on_click(cx.listener(|this,_,_,cx| cx.write_to_clipboard(ClipboardItem::new_string(this.status.clone())))))
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
                cx.new(|_| IdentityWindow {
                    profile,
                    page: Page::Identity,
                    fields: Vec::new(),
                    status: "Créez une identité ou affichez celle du profil sélectionné.".into(),
                    busy: false,
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
