//! Public chat transport. No token is placed in a URL or persisted in the cache.
use crate::{Cache, Instance, Result, Session};
use kova_protocol::{ChatMessage, ChatPage};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};

#[derive(Clone)]
pub struct ChatScope {
    pub origin: String,
    pub identity: String,
    pub channel: String,
}
impl ChatScope {
    pub fn new(origin: &str, identity: &str, channel: &str) -> Result<Self> {
        crate::validate_origin(origin)?;
        if !kova_protocol::valid_uuid(channel) || identity.is_empty() {
            return Err("Salon ou identité invalide".into());
        }
        Ok(Self {
            origin: origin.into(),
            identity: identity.into(),
            channel: channel.into(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingMessage {
    pub client_id: String,
    pub body: String,
}
impl Instance {
    pub fn chat_scope(&self, session: &Session, channel: &str) -> Result<ChatScope> {
        self.authorize(session)?;
        ChatScope::new(
            &self.origin,
            session.profile["global_identity_id"]
                .as_str()
                .ok_or("Identité de session absente")?,
            channel,
        )
    }
    pub fn messages(&self, session: &Session, channel: &str, after: i64) -> Result<ChatPage> {
        self.chat_scope(session, channel)?;
        if after < 0 {
            return Err("Curseur invalide".into());
        }
        Ok(serde_json::from_value(self.community_request(
            session,
            Method::GET,
            &format!("/channels/{channel}/messages?after={after}"),
            None,
        )?)?)
    }
    pub fn send_message(
        &self,
        session: &Session,
        channel: &str,
        pending: &PendingMessage,
    ) -> Result<ChatMessage> {
        self.chat_scope(session, channel)?;
        if !kova_protocol::valid_uuid(&pending.client_id)
            || !kova_protocol::valid_message_body(&pending.body)
        {
            return Err("Message invalide (1–4000 caractères)".into());
        }
        let message: ChatMessage = serde_json::from_value(self.community_request(
            session,
            Method::POST,
            &format!("/channels/{channel}/messages"),
            Some(json!(pending)),
        )?)?;
        if message.channel_id != channel
            || message.client_id != pending.client_id
            || message.body != pending.body
            || session.profile["id"] != message.author_id
        {
            return Err("Accusé d'envoi incohérent".into());
        }
        Ok(message)
    }
    pub fn sync_messages(&self, session: &Session, cache: &Cache, channel: &str) -> Result<()> {
        let scope = self.chat_scope(session, channel)?;
        loop {
            let page = self.messages(session, channel, cache.chat_cursor(&scope)?)?;
            cache.merge_chat_page(&scope, &page)?;
            if page.next_seq == page.last_seq {
                return Ok(());
            }
        }
    }
    pub fn subscribe_chat(
        &self,
        session: &Session,
        channel: &str,
        after: i64,
    ) -> Result<ChatStream> {
        self.chat_scope(session, channel)?;
        if after < 0 {
            return Err("Curseur invalide".into());
        }
        let mut url = crate::validate_origin(&self.origin)?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).map_err(|_| "Schéma WS invalide")?;
        url.set_path(&format!("/api/v1/channels/{channel}/events"));
        url.set_query(Some(&format!("after={after}")));
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert("Origin", self.origin.parse()?);
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", session.token).parse()?,
        );
        let host = url
            .host_str()
            .ok_or("Hôte absent")?
            .trim_matches(['[', ']']);
        let addresses =
            (host, url.port_or_known_default().ok_or("Port absent")?).to_socket_addrs()?;
        let mut connection = None;
        for address in addresses {
            if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(3)) {
                connection = Some(stream);
                break;
            }
        }
        let stream = connection.ok_or("Connexion chat impossible")?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(3_000_000))
            .max_frame_size(Some(3_000_000));
        let (socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)?;
        Ok(ChatStream(socket))
    }
}

pub struct ChatStream(WebSocket<MaybeTlsStream<TcpStream>>);
impl ChatStream {
    /// Empty pages are authenticated heartbeats. A closed socket must resume from the cache.
    pub fn read_page(&mut self) -> Result<Option<ChatPage>> {
        match self.0.read()? {
            Message::Text(text) => Ok(Some(serde_json::from_str(&text)?)),
            Message::Ping(_) | Message::Pong(_) => {
                self.0.flush()?;
                Ok(None)
            }
            Message::Close(frame) if frame.as_ref().is_some_and(|f| u16::from(f.code) == 1008) => {
                Err("Accès au chat retiré ou session expirée ; reconnectez-vous.".into())
            }
            _ => Err("Flux chat interrompu".into()),
        }
    }
}
