//! The multiplexer: many LSP clients over one backend, as a state machine with no I/O.
//!
//! Each call takes one message (or event) and returns the messages to send. Payloads stay raw
//! ([`Message`]) except where a rule below rewrites them. Rules:
//!
//! - **Ids.** A client request goes to the backend under a fresh backend id; its response goes
//!   back to that client only, under the client's own id (exactly, as `serde_json` keeps it).
//!   A backend request goes to one client (the oldest attached) under a fresh proxy id and its
//!   answer goes back under the backend's id.
//! - **Lifecycle.** The first `initialize` is forwarded (with the worktree as root, the daemon as
//!   `processId`, and the configured settings merged in); later clients get the cached result.
//!   Each client must send its own `initialize` before other requests. `initialized` is forwarded
//!   once; `shutdown` is answered here; `exit` only detaches the client. The backend outlives
//!   any one client.
//! - **Documents.** `didOpen`/`didClose` are reference-counted per URI; the advertised sync kind is
//!   rewritten to *full*, so every change carries the whole text and the last buffer wins;
//!   versions are rewritten to one increasing counter per URI, and the backend's
//!   `publishDiagnostics.version` (on that counter) is dropped rather than shown to clients.
//! - **Cancellation and exits.** `$/cancelRequest` is forwarded under the backend id; a detached
//!   client's requests are cancelled and their late responses dropped, its documents closed, and
//!   backend requests routed to it answered here.
//! - **Progress.** Client `workDoneToken`/`partialResultToken`s are stripped (two clients may pick
//!   the same token); server-created progress goes to the client that accepted the token.
//! - **Edits.** A backend `workspace/applyEdit` is answered here with `applied: false`: the
//!   proxy cannot tell which client caused it, and agents write files themselves.
//! - **Undecodable messages.** A client message that does not decode gets a parse error (under
//!   its id when one can be found); a backend message that does not decode fails the client
//!   request it answers, so no client waits for ever.
//!
//! Pinned by the tests below and by `tests/session.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::json::{Message, Value, json};

/// A client's identity within one session.
pub type ClientId = u64;

/// JSON-RPC error codes used here.
pub mod codes {
    /// The message is not JSON-RPC.
    pub const PARSE_ERROR: i32 = -32700;
    /// The request is malformed (a null or duplicate id).
    pub const INVALID_REQUEST: i32 = -32600;
    /// The client has not sent `initialize` yet.
    pub const SERVER_NOT_INITIALIZED: i32 = -32002;
    /// The backend failed or went away.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// A backend request no client could answer.
    pub const REQUEST_FAILED: i32 = -32803;
}

/// What to do after a step.
#[derive(Debug, Clone)]
pub enum Output {
    /// Send to the backend.
    Backend(Message),
    /// Send to one client.
    Client(ClientId, Message),
    /// Close one client's connection (it sent `exit`).
    Close(ClientId),
    /// The backend answered the proxy's own `shutdown`.
    ShutdownAcknowledged,
}

/// Session settings.
#[derive(Debug, Clone)]
pub struct MuxConfig {
    /// The worktree root as a `file://` URI.
    pub root_uri: String,
    /// The worktree root as a path string.
    pub root_path: String,
    /// The worktree folder's name.
    pub root_name: String,
    /// The `processId` the backend watches (the daemon's), so it exits with the daemon.
    pub process_id: Option<u32>,
    /// Merged into `initializationOptions.settings` (import switches and exclusions).
    pub settings: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IdKey {
    Num(String),
    Str(String),
}

fn id_key(id: &Value) -> Option<IdKey> {
    match id {
        Value::Number(n) => Some(IdKey::Num(n.to_string())),
        Value::String(s) => Some(IdKey::Str(s.clone())),
        _ => None,
    }
}

enum Origin {
    Client { client: ClientId, id: Value },
    Orphaned,
    Initialize,
    Shutdown,
}

enum Init {
    NotSent,
    InFlight(Vec<(ClientId, Value)>),
    Done(Value),
}

struct ServerRequest {
    client: ClientId,
    backend_id: Value,
    method: String,
    params: Option<Value>,
}

struct Doc {
    owners: BTreeMap<ClientId, u32>,
    version: u64,
}

/// The multiplexer of one backend.
pub struct Mux {
    config: MuxConfig,
    next_id: u64,
    clients: BTreeSet<ClientId>,
    /// Clients whose own `initialize` has been answered.
    ready: BTreeSet<ClientId>,
    pending: HashMap<u64, Origin>,
    forwarded: HashMap<(ClientId, IdKey), u64>,
    server_requests: HashMap<u64, ServerRequest>,
    init: Init,
    initialized_sent: bool,
    docs: HashMap<String, Doc>,
    progress: HashMap<IdKey, ClientId>,
}

impl Mux {
    /// A multiplexer with no clients and an uninitialized backend.
    #[must_use]
    pub fn new(config: MuxConfig) -> Self {
        Self {
            config,
            next_id: 1,
            clients: BTreeSet::new(),
            ready: BTreeSet::new(),
            pending: HashMap::new(),
            forwarded: HashMap::new(),
            server_requests: HashMap::new(),
            init: Init::NotSent,
            initialized_sent: false,
            docs: HashMap::new(),
            progress: HashMap::new(),
        }
    }

    /// Registers a client.
    pub fn attach(&mut self, client: ClientId) {
        self.clients.insert(client);
    }

    /// How many clients are attached.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// How many requests await the backend (orphaned ones included).
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// A client message that did not decode; `id` is what a scan of it found.
    pub fn client_undecodable(&mut self, client: ClientId, id: Option<Value>) -> Vec<Output> {
        if !self.clients.contains(&client) {
            return Vec::new();
        }
        let id = id.unwrap_or(Value::Null);
        vec![Output::Client(
            client,
            Message::error(id, codes::PARSE_ERROR, "message does not decode"),
        )]
    }

    /// A backend message that did not decode; `id` is what a scan of it found. The client
    /// request it answers gets an internal error.
    pub fn backend_undecodable(&mut self, id: Option<Value>) -> Vec<Output> {
        let Some(bid) = id.as_ref().and_then(Value::as_u64) else {
            return Vec::new();
        };
        match self.pending.remove(&bid) {
            Some(Origin::Client { client, id }) => {
                if let Some(key) = id_key(&id) {
                    self.forwarded.remove(&(client, key));
                }
                let msg = "jdtls sent an answer that does not decode";
                vec![Output::Client(
                    client,
                    Message::error(id, codes::INTERNAL_ERROR, msg),
                )]
            }
            Some(Origin::Initialize) => {
                let waiting = match std::mem::replace(&mut self.init, Init::NotSent) {
                    Init::InFlight(waiting) => waiting,
                    other => {
                        self.init = other;
                        return Vec::new();
                    }
                };
                let msg = "jdtls sent an initialize result that does not decode";
                waiting
                    .into_iter()
                    .map(|(c, id)| {
                        Output::Client(c, Message::error(id, codes::INTERNAL_ERROR, msg))
                    })
                    .collect()
            }
            Some(Origin::Shutdown) => vec![Output::ShutdownAcknowledged],
            Some(Origin::Orphaned) | None => Vec::new(),
        }
    }

    /// One message from a client.
    pub fn from_client(&mut self, client: ClientId, mut msg: Message) -> Vec<Output> {
        if !self.clients.contains(&client) {
            return Vec::new();
        }
        match (msg.method.clone(), msg.id.clone()) {
            (Some(method), Some(id)) => self.client_request(client, &method, id, msg),
            (Some(method), None) => self.client_notification(client, &method, msg),
            (None, Some(id)) => {
                let Some(pid) = id.as_u64() else {
                    return Vec::new();
                };
                match self.server_requests.remove(&pid) {
                    Some(req) if req.client == client => {
                        msg.id = Some(req.backend_id);
                        vec![Output::Backend(msg)]
                    }
                    Some(req) => {
                        self.server_requests.insert(pid, req);
                        Vec::new()
                    }
                    None => Vec::new(),
                }
            }
            (None, None) => Vec::new(),
        }
    }

    fn client_request(
        &mut self,
        client: ClientId,
        method: &str,
        id: Value,
        mut msg: Message,
    ) -> Vec<Output> {
        match method {
            "initialize" => return self.client_initialize(client, id, &msg),
            "shutdown" => return vec![Output::Client(client, Message::response(id, &Value::Null))],
            _ => {}
        }
        if !matches!(self.init, Init::Done(_)) || !self.ready.contains(&client) {
            let err = Message::error(id, codes::SERVER_NOT_INITIALIZED, "server not initialized");
            return vec![Output::Client(client, err)];
        }
        let Some(key) = id_key(&id) else {
            return vec![Output::Client(
                client,
                Message::error(id, codes::INVALID_REQUEST, "invalid id"),
            )];
        };
        if self.forwarded.contains_key(&(client, key.clone())) {
            return vec![Output::Client(
                client,
                Message::error(id, codes::INVALID_REQUEST, "duplicate id"),
            )];
        }
        let has_token = msg.params.as_ref().is_some_and(|p| {
            p.get().contains("workDoneToken") || p.get().contains("partialResultToken")
        });
        if has_token && let Some(mut params) = msg.params_value() {
            if let Some(map) = params.as_object_mut() {
                map.remove("workDoneToken");
                map.remove("partialResultToken");
            }
            msg.set_params(&params);
        }
        let bid = self.fresh_id();
        self.pending.insert(bid, Origin::Client { client, id });
        self.forwarded.insert((client, key), bid);
        msg.id = Some(Value::from(bid));
        vec![Output::Backend(msg)]
    }

    fn client_initialize(&mut self, client: ClientId, id: Value, msg: &Message) -> Vec<Output> {
        match &mut self.init {
            Init::Done(result) => {
                let reply = Message::response(id, result);
                self.ready.insert(client);
                vec![Output::Client(client, reply)]
            }
            Init::InFlight(waiting) => {
                waiting.push((client, id));
                Vec::new()
            }
            Init::NotSent => {
                let mut params = match msg.params_value() {
                    Some(v @ Value::Object(_)) => v,
                    _ => json!({}),
                };
                if let Some(map) = params.as_object_mut() {
                    map.insert(
                        "processId".into(),
                        self.config.process_id.map_or(Value::Null, Value::from),
                    );
                    map.insert("rootUri".into(), json!(self.config.root_uri));
                    map.insert("rootPath".into(), json!(self.config.root_path));
                    map.insert(
                        "workspaceFolders".into(),
                        json!([{ "uri": self.config.root_uri, "name": self.config.root_name }]),
                    );
                    map.remove("workDoneToken");
                    if let Some(settings) = &self.config.settings {
                        let wanted = json!({ "settings": settings });
                        match map.get_mut("initializationOptions") {
                            Some(existing @ Value::Object(_)) => merge(existing, wanted),
                            _ => {
                                map.insert("initializationOptions".into(), wanted);
                            }
                        }
                    }
                }
                let bid = self.fresh_id();
                self.pending.insert(bid, Origin::Initialize);
                self.init = Init::InFlight(vec![(client, id)]);
                vec![Output::Backend(Message::request(
                    Value::from(bid),
                    "initialize",
                    &params,
                ))]
            }
        }
    }

    fn client_notification(
        &mut self,
        client: ClientId,
        method: &str,
        mut msg: Message,
    ) -> Vec<Output> {
        let initialized = matches!(self.init, Init::Done(_));
        match method {
            "exit" => {
                let mut out = self.detach(client);
                out.push(Output::Close(client));
                out
            }
            "initialized" => {
                if initialized && !self.initialized_sent {
                    self.initialized_sent = true;
                    vec![Output::Backend(msg)]
                } else {
                    Vec::new()
                }
            }
            "$/cancelRequest" => {
                let key = msg
                    .params_value()
                    .and_then(|p| p.get("id").and_then(id_key));
                match key.and_then(|k| self.forwarded.get(&(client, k))) {
                    Some(bid) => vec![Output::Backend(cancel(*bid))],
                    None => Vec::new(),
                }
            }
            "textDocument/didOpen" if initialized => self.did_open(client, &msg),
            "textDocument/didChange" => {
                let Some(mut params) = msg.params_value() else {
                    return Vec::new();
                };
                let Some(uri) = text_document_uri(&params) else {
                    return Vec::new();
                };
                let Some(doc) = self.docs.get_mut(&uri) else {
                    return Vec::new();
                };
                if !doc.owners.contains_key(&client) {
                    return Vec::new();
                }
                doc.version = doc.version.saturating_add(1);
                if let Some(td) = params
                    .get_mut("textDocument")
                    .and_then(Value::as_object_mut)
                {
                    td.insert("version".into(), Value::from(doc.version));
                }
                msg.set_params(&params);
                vec![Output::Backend(msg)]
            }
            "textDocument/didClose" => {
                let Some(uri) = msg.params_value().as_ref().and_then(text_document_uri) else {
                    return Vec::new();
                };
                if self.release(client, &uri) {
                    vec![Output::Backend(msg)]
                } else {
                    Vec::new()
                }
            }
            _ if initialized => vec![Output::Backend(msg)],
            _ => Vec::new(),
        }
    }

    fn did_open(&mut self, client: ClientId, msg: &Message) -> Vec<Output> {
        let Some(mut params) = msg.params_value() else {
            return Vec::new();
        };
        let Some(uri) = text_document_uri(&params) else {
            return Vec::new();
        };
        match self.docs.get_mut(&uri) {
            Some(doc) => {
                let count = doc.owners.entry(client).or_insert(0);
                *count = count.saturating_add(1);
                doc.version = doc.version.saturating_add(1);
                let text = params
                    .pointer("/textDocument/text")
                    .cloned()
                    .unwrap_or_else(|| json!(""));
                let change = json!({
                    "textDocument": { "uri": uri, "version": doc.version },
                    "contentChanges": [{ "text": text }],
                });
                vec![Output::Backend(Message::notification(
                    "textDocument/didChange",
                    &change,
                ))]
            }
            None => {
                let mut owners = BTreeMap::new();
                owners.insert(client, 1);
                self.docs.insert(uri, Doc { owners, version: 1 });
                if let Some(td) = params
                    .get_mut("textDocument")
                    .and_then(Value::as_object_mut)
                {
                    td.insert("version".into(), Value::from(1u64));
                }
                vec![Output::Backend(Message::notification(
                    "textDocument/didOpen",
                    &params,
                ))]
            }
        }
    }

    /// Drops one of `client`'s references to `uri`; true when the backend should close it.
    fn release(&mut self, client: ClientId, uri: &str) -> bool {
        let Some(doc) = self.docs.get_mut(uri) else {
            return false;
        };
        let Some(count) = doc.owners.get_mut(&client) else {
            return false;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            doc.owners.remove(&client);
        }
        if doc.owners.is_empty() {
            self.docs.remove(uri);
            return true;
        }
        false
    }

    /// One message from the backend.
    pub fn from_backend(&mut self, mut msg: Message) -> Vec<Output> {
        match (msg.method.clone(), msg.id.clone()) {
            (None, Some(id)) => self.backend_response(&id, msg),
            (Some(method), Some(id)) => {
                let params = msg.params_value();
                let client = self.clients.first().copied();
                let Some(client) = client.filter(|_| method != "workspace/applyEdit") else {
                    return vec![Output::Backend(local_answer(&method, id, params.as_ref()))];
                };
                let pid = self.fresh_id();
                let token = params
                    .as_ref()
                    .and_then(|p| p.get("token"))
                    .and_then(id_key);
                if let (true, Some(key)) = (method == "window/workDoneProgress/create", token) {
                    self.progress.insert(key, client);
                }
                self.server_requests.insert(
                    pid,
                    ServerRequest {
                        client,
                        backend_id: id,
                        method,
                        params,
                    },
                );
                msg.id = Some(Value::from(pid));
                vec![Output::Client(client, msg)]
            }
            (Some(method), None) if method == "$/progress" => {
                let params = msg.params_value();
                let Some(key) = params
                    .as_ref()
                    .and_then(|p| p.get("token"))
                    .and_then(id_key)
                else {
                    return Vec::new();
                };
                let ended = params
                    .as_ref()
                    .and_then(|p| p.pointer("/value/kind"))
                    .and_then(Value::as_str)
                    == Some("end");
                let target = if ended {
                    self.progress.remove(&key)
                } else {
                    self.progress.get(&key).copied()
                };
                target
                    .map(|c| vec![Output::Client(c, msg)])
                    .unwrap_or_default()
            }
            (Some(method), None) => {
                if method == "textDocument/publishDiagnostics"
                    && let Some(mut params) = msg.params_value()
                    && let Some(map) = params.as_object_mut()
                    && map.remove("version").is_some()
                {
                    msg.set_params(&params);
                }
                self.clients
                    .iter()
                    .map(|&c| Output::Client(c, msg.clone()))
                    .collect()
            }
            (None, None) => Vec::new(),
        }
    }

    fn backend_response(&mut self, id: &Value, mut msg: Message) -> Vec<Output> {
        let Some(bid) = id.as_u64() else {
            return Vec::new();
        };
        match self.pending.remove(&bid) {
            Some(Origin::Client { client, id }) => {
                if let Some(key) = id_key(&id) {
                    self.forwarded.remove(&(client, key));
                }
                msg.id = Some(id);
                vec![Output::Client(client, msg)]
            }
            Some(Origin::Initialize) => {
                let waiting = match std::mem::replace(&mut self.init, Init::NotSent) {
                    Init::InFlight(waiting) => waiting,
                    other => {
                        self.init = other;
                        return Vec::new();
                    }
                };
                if msg.error.is_some() {
                    return waiting
                        .into_iter()
                        .map(|(c, id)| {
                            let mut reply = msg.clone();
                            reply.id = Some(id);
                            Output::Client(c, reply)
                        })
                        .collect();
                }
                let mut result = msg.result_value().unwrap_or(Value::Null);
                force_full_sync(&mut result);
                self.ready.extend(waiting.iter().map(|(c, _)| *c));
                let out = waiting
                    .into_iter()
                    .map(|(c, id)| Output::Client(c, Message::response(id, &result)))
                    .collect();
                self.init = Init::Done(result);
                out
            }
            Some(Origin::Shutdown) => vec![Output::ShutdownAcknowledged],
            Some(Origin::Orphaned) | None => Vec::new(),
        }
    }

    /// Detaches a client: cancels its requests, answers backend requests routed to it, closes
    /// the documents only it held. Pinned by `tests::should_cancel_and_close_when_a_client_detaches`.
    pub fn detach(&mut self, client: ClientId) -> Vec<Output> {
        self.ready.remove(&client);
        if !self.clients.remove(&client) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut cancelled: Vec<u64> = Vec::new();
        self.forwarded.retain(|(c, _), bid| {
            if *c == client {
                cancelled.push(*bid);
                false
            } else {
                true
            }
        });
        cancelled.sort_unstable();
        for bid in cancelled {
            self.pending.insert(bid, Origin::Orphaned);
            out.push(Output::Backend(cancel(bid)));
        }
        if let Init::InFlight(waiting) = &mut self.init {
            waiting.retain(|(c, _)| *c != client);
        }
        let mut routed: Vec<u64> = self
            .server_requests
            .iter()
            .filter(|(_, r)| r.client == client)
            .map(|(pid, _)| *pid)
            .collect();
        routed.sort_unstable();
        for pid in routed {
            if let Some(req) = self.server_requests.remove(&pid) {
                out.push(Output::Backend(local_answer(
                    &req.method,
                    req.backend_id,
                    req.params.as_ref(),
                )));
            }
        }
        self.progress.retain(|_, c| *c != client);
        let mut uris: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| d.owners.contains_key(&client))
            .map(|(u, _)| u.clone())
            .collect();
        uris.sort();
        for uri in uris {
            if let Some(doc) = self.docs.get_mut(&uri) {
                doc.owners.remove(&client);
                if doc.owners.is_empty() {
                    self.docs.remove(&uri);
                    let params = json!({ "textDocument": { "uri": uri } });
                    out.push(Output::Backend(Message::notification(
                        "textDocument/didClose",
                        &params,
                    )));
                }
            }
        }
        out
    }

    /// The proxy's own `shutdown` request, sent when the backend is evicted or the daemon stops.
    pub fn shutdown_request(&mut self) -> Message {
        let bid = self.fresh_id();
        self.pending.insert(bid, Origin::Shutdown);
        Message::request(Value::from(bid), "shutdown", &Value::Null)
    }

    /// The backend went away: every client request still waiting gets an error.
    pub fn backend_gone(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        let mut pending: Vec<(u64, Origin)> = self.pending.drain().collect();
        pending.sort_unstable_by_key(|(bid, _)| *bid);
        for (_, origin) in pending {
            if let Origin::Client { client, id } = origin {
                out.push(Output::Client(
                    client,
                    Message::error(id, codes::INTERNAL_ERROR, "jdtls exited"),
                ));
            }
        }
        if let Init::InFlight(waiting) = std::mem::replace(&mut self.init, Init::NotSent) {
            for (client, id) in waiting {
                out.push(Output::Client(
                    client,
                    Message::error(id, codes::INTERNAL_ERROR, "jdtls exited"),
                ));
            }
        }
        self.forwarded.clear();
        out
    }
}

/// The `exit` notification.
#[must_use]
pub fn exit_notification() -> Message {
    Message::notification("exit", &Value::Null)
}

fn text_document_uri(params: &Value) -> Option<String> {
    params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn force_full_sync(result: &mut Value) {
    let Some(caps) = result.get_mut("capabilities") else {
        return;
    };
    match caps.get_mut("textDocumentSync") {
        Some(kind @ Value::Number(_)) => *kind = Value::from(1u64),
        Some(Value::Object(sync)) => {
            sync.insert("change".into(), Value::from(1u64));
        }
        _ => {}
    }
}

fn merge(dst: &mut Value, src: Value) {
    match (dst, src) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                match d.get_mut(&k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        d.insert(k, v);
                    }
                }
            }
        }
        (dst, src) => *dst = src,
    }
}

fn local_answer(method: &str, id: Value, params: Option<&Value>) -> Message {
    match method {
        "workspace/configuration" => {
            let n = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Message::response(id, &Value::Array(vec![Value::Null; n]))
        }
        "workspace/applyEdit" => Message::response(id, &json!({ "applied": false })),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "window/showMessageRequest" => Message::response(id, &Value::Null),
        _ => Message::error(id, codes::REQUEST_FAILED, "no client attached"),
    }
}

fn cancel(bid: u64) -> Message {
    Message::notification("$/cancelRequest", &json!({ "id": bid }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Message {
        crate::json::parse(s.as_bytes()).unwrap()
    }

    /// A message as a JSON value, for assertions.
    fn v(m: &Message) -> Value {
        serde_json::from_slice(&crate::json::encode(m).unwrap()).unwrap()
    }

    fn mux() -> Mux {
        Mux::new(MuxConfig {
            root_uri: "file:///w/repo".into(),
            root_path: "/w/repo".into(),
            root_name: "repo".into(),
            process_id: Some(42),
            settings: Some(json!({"java":{"import":{"gradle":{"enabled":false}}}})),
        })
    }

    fn backend(out: &[Output]) -> Vec<Value> {
        out.iter()
            .filter_map(|o| {
                if let Output::Backend(m) = o {
                    Some(v(m))
                } else {
                    None
                }
            })
            .collect()
    }

    fn to_client(out: &[Output], c: ClientId) -> Vec<Value> {
        out.iter()
            .filter_map(|o| match o {
                Output::Client(x, m) if *x == c => Some(v(m)),
                _ => None,
            })
            .collect()
    }

    fn respond(to: &Value, body: &str) -> Message {
        let mut m = parse(body);
        m.id = to.get("id").cloned();
        m
    }

    /// Two clients attached to an initialized backend.
    fn initialized_pair() -> Mux {
        let mut m = mux();
        m.attach(1);
        m.attach(2);
        let out = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"processId":7,"initializationOptions":{"settings":{"java":{"x":1}}}}}"#));
        let init = backend(&out)[0].clone();
        assert!(
            m.from_client(
                2,
                parse(r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{}}"#)
            )
            .is_empty()
        );
        let out = m.from_backend(respond(&init, r#"{"jsonrpc":"2.0","result":{"capabilities":{"textDocumentSync":{"openClose":true,"change":2}}}}"#));
        assert_eq!(to_client(&out, 1).len(), 1);
        assert_eq!(to_client(&out, 2).len(), 1);
        m
    }

    #[test]
    fn should_forward_initialize_once_with_the_worktree_root_when_two_clients_initialize() {
        // given
        let mut m = mux();
        m.attach(1);
        m.attach(2);

        // when
        let first = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"processId":7,"rootUri":"file:///elsewhere"}}"#));
        let second = m.from_client(
            2,
            parse(r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{}}"#),
        );

        // then
        let sent = backend(&first);
        assert_eq!(sent.len(), 1);
        let params = &sent[0]["params"];
        assert_eq!(params["rootUri"], "file:///w/repo");
        assert_eq!(params["processId"], 42);
        assert_eq!(
            params["initializationOptions"]["settings"]["java"]["import"]["gradle"]["enabled"],
            false
        );
        assert!(second.is_empty());
    }

    #[test]
    fn should_answer_each_client_under_its_own_id_and_force_full_sync_when_initialized() {
        // given
        let mut m = initialized_pair();

        // when: a third client arrives after the result is cached
        m.attach(3);
        let out = m.from_client(
            3,
            parse(r#"{"jsonrpc":"2.0","id":5,"method":"initialize","params":{}}"#),
        );

        // then
        let reply = &to_client(&out, 3)[0];
        assert_eq!(reply["id"], 5);
        assert_eq!(
            reply["result"]["capabilities"]["textDocumentSync"]["change"],
            1
        );
        assert!(backend(&out).is_empty());
    }

    #[test]
    fn should_keep_settings_the_client_gave_when_merging_configured_settings() {
        // given
        let mut m = mux();
        m.attach(1);

        // when
        let out = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"initializationOptions":{"settings":{"java":{"x":1}}}}}"#));

        // then
        let java = &backend(&out)[0]["params"]["initializationOptions"]["settings"]["java"];
        assert_eq!(java["x"], 1);
        assert!(java.get("import").is_some());
    }

    #[test]
    fn should_route_each_response_to_its_client_with_its_own_id_when_ids_collide() {
        // given
        let mut m = initialized_pair();

        // when: both clients use id 1; a's id is beyond f64's exact range
        let a = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":9007199254740993,"method":"textDocument/hover","params":{"who":"a","workDoneToken":"t"}}"#));
        let b = m.from_client(2, parse(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/hover","params":{"who":"b","n":123456789012345678901234567890}}"#));
        let (ba, bb) = (backend(&a)[0].clone(), backend(&b)[0].clone());
        let out_b = m.from_backend(respond(&bb, r#"{"jsonrpc":"2.0","result":"for b"}"#));
        let out_a = m.from_backend(respond(&ba, r#"{"jsonrpc":"2.0","result":"for a"}"#));

        // then
        assert_ne!(ba["id"], bb["id"]);
        assert!(ba["params"].get("workDoneToken").is_none());
        let Output::Backend(raw_b) = &b[0] else {
            panic!()
        };
        assert!(
            raw_b
                .params
                .as_ref()
                .unwrap()
                .get()
                .contains("123456789012345678901234567890")
        );
        let Output::Client(1, ra) = &out_a[0] else {
            panic!()
        };
        assert_eq!(
            String::from_utf8(crate::json::encode(ra).unwrap()).unwrap(),
            r#"{"jsonrpc":"2.0","id":9007199254740993,"result":"for a"}"#
        );
        assert_eq!(
            to_client(&out_b, 2),
            vec![json!({"jsonrpc":"2.0","id":1,"result":"for b"})]
        );
    }

    #[test]
    fn should_answer_shutdown_locally_and_detach_on_exit_when_a_client_leaves() {
        // given
        let mut m = initialized_pair();

        // when
        let shut = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#));
        let exit = m.from_client(1, parse(r#"{"jsonrpc":"2.0","method":"exit"}"#));

        // then
        assert_eq!(
            to_client(&shut, 1),
            vec![json!({"jsonrpc":"2.0","id":3,"result":null})]
        );
        assert!(matches!(exit.as_slice(), [Output::Close(1)]));
        assert_eq!(m.client_count(), 1);
    }

    #[test]
    fn should_forward_cancel_under_the_backend_id_when_a_client_cancels() {
        // given
        let mut m = initialized_pair();
        let sent = m.from_client(2, parse(r#"{"jsonrpc":"2.0","id":"r","method":"x"}"#));
        let bid = backend(&sent)[0]["id"].clone();

        // when
        let out = m.from_client(
            2,
            parse(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":"r"}}"#),
        );
        let other = m.from_client(
            1,
            parse(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":"r"}}"#),
        );

        // then
        assert_eq!(backend(&out)[0]["params"]["id"], bid);
        assert!(other.is_empty());
    }

    #[test]
    fn should_cancel_and_close_when_a_client_detaches() {
        // given
        let mut m = initialized_pair();
        let sent = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":4,"method":"slow"}"#));
        let req = backend(&sent)[0].clone();
        m.from_client(1, parse(r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a","version":1,"text":"x"}}}"#));
        m.from_client(1, parse(r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///b","version":1,"text":"x"}}}"#));
        m.from_client(2, parse(r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///b","version":1,"text":"y"}}}"#));

        // when
        let out = m.detach(1);
        let late_out = m.from_backend(respond(&req, r#"{"jsonrpc":"2.0","result":1}"#));

        // then: the request is cancelled, only the document no one else holds is closed
        let sent = backend(&out);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["method"], "$/cancelRequest");
        assert_eq!(sent[0]["params"]["id"], req["id"]);
        assert_eq!(sent[1]["method"], "textDocument/didClose");
        assert_eq!(sent[1]["params"]["textDocument"]["uri"], "file:///a");
        assert!(late_out.is_empty());
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn should_reference_count_documents_and_rewrite_versions_when_two_clients_open_one_uri() {
        // given
        let mut m = initialized_pair();
        let open = |text: &str| {
            parse(&format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///a","languageId":"java","version":1,"text":"{text}"}}}}}}"#
            ))
        };
        let close = || {
            parse(
                r#"{"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///a"}}}"#,
            )
        };

        // when
        let o1 = m.from_client(1, open("one"));
        let o2 = m.from_client(2, open("two"));
        let ch = m.from_client(1, parse(r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///a","version":2},"contentChanges":[{"text":"three"}]}}"#));
        let c1 = m.from_client(1, close());
        let c2 = m.from_client(2, close());
        let c3 = m.from_client(2, close());

        // then
        assert_eq!(backend(&o1)[0]["method"], "textDocument/didOpen");
        let reopen = &backend(&o2)[0];
        assert_eq!(reopen["method"], "textDocument/didChange");
        assert_eq!(reopen["params"]["textDocument"]["version"], 2);
        assert_eq!(reopen["params"]["contentChanges"][0]["text"], "two");
        assert_eq!(backend(&ch)[0]["params"]["textDocument"]["version"], 3);
        assert!(c1.is_empty());
        assert_eq!(backend(&c2)[0]["method"], "textDocument/didClose");
        assert!(c3.is_empty());
    }

    #[test]
    fn should_route_backend_requests_to_one_client_and_answer_them_when_it_leaves() {
        // given
        let mut m = initialized_pair();

        // when
        let out = m.from_backend(parse(r#"{"jsonrpc":"2.0","id":77,"method":"workspace/configuration","params":{"items":[{},{}]}}"#));
        let routed = to_client(&out, 1)[0].clone();
        let gone = m.detach(1);

        // then
        assert!(to_client(&out, 2).is_empty());
        assert_ne!(routed["id"], 77);
        let answer = &backend(&gone)[0];
        assert_eq!(answer["id"], 77);
        assert_eq!(answer["result"], json!([null, null]));
    }

    #[test]
    fn should_answer_apply_edit_locally_when_the_backend_asks() {
        // given
        let mut m = initialized_pair();

        // when
        let out = m.from_backend(parse(
            r#"{"jsonrpc":"2.0","id":5,"method":"workspace/applyEdit","params":{"edit":{}}}"#,
        ));

        // then
        assert!(to_client(&out, 1).is_empty() && to_client(&out, 2).is_empty());
        assert_eq!(backend(&out)[0]["result"], json!({"applied": false}));
    }

    #[test]
    fn should_return_the_answer_under_the_backend_id_when_the_client_replies() {
        // given
        let mut m = initialized_pair();
        let out = m.from_backend(parse(
            r#"{"jsonrpc":"2.0","id":"s1","method":"client/registerCapability","params":{}}"#,
        ));
        let routed = to_client(&out, 1)[0].clone();

        // when
        let wrong = m.from_client(2, respond(&routed, r#"{"jsonrpc":"2.0","result":null}"#));
        let right = m.from_client(1, respond(&routed, r#"{"jsonrpc":"2.0","result":null}"#));

        // then
        assert!(wrong.is_empty());
        assert_eq!(backend(&right)[0]["id"], "s1");
    }

    #[test]
    fn should_broadcast_notifications_and_route_progress_to_its_owner_when_the_backend_reports() {
        // given
        let mut m = initialized_pair();
        m.from_backend(parse(r#"{"jsonrpc":"2.0","id":8,"method":"window/workDoneProgress/create","params":{"token":"tk"}}"#));

        // when
        let diags = m.from_backend(parse(r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///a","version":7,"diagnostics":[]}}"#));
        let progress = m.from_backend(parse(r#"{"jsonrpc":"2.0","method":"$/progress","params":{"token":"tk","value":{"kind":"end"}}}"#));
        let after_end = m.from_backend(parse(r#"{"jsonrpc":"2.0","method":"$/progress","params":{"token":"tk","value":{"kind":"report"}}}"#));

        // then
        let (d1, d2) = (to_client(&diags, 1), to_client(&diags, 2));
        assert_eq!(d1.len() + d2.len(), 2);
        assert!(d1[0]["params"].get("version").is_none());
        assert_eq!(to_client(&progress, 1).len(), 1);
        assert!(to_client(&progress, 2).is_empty());
        assert!(after_end.is_empty());
    }

    #[test]
    fn should_refuse_requests_before_initialize_and_answer_errors_when_backend_gone() {
        // given
        let mut m = mux();
        m.attach(1);

        // when
        let early = m.from_client(1, parse(r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#));
        m.from_client(
            1,
            parse(r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#),
        );
        let gone = m.backend_gone();

        // then
        assert_eq!(to_client(&early, 1)[0]["error"]["code"], -32002);
        assert_eq!(to_client(&gone, 1)[0]["error"]["code"], -32603);
    }

    #[test]
    fn should_refuse_a_client_that_skipped_initialize_when_another_client_initialized() {
        // given
        let mut m = initialized_pair();
        m.attach(3);

        // when
        let early = m.from_client(3, parse(r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#));

        // then
        assert_eq!(to_client(&early, 3)[0]["error"]["code"], -32002);
        assert!(backend(&early).is_empty());
    }

    #[test]
    fn should_fail_the_waiting_request_when_its_answer_does_not_decode() {
        // given
        let mut m = initialized_pair();
        let sent = m.from_client(2, parse(r#"{"jsonrpc":"2.0","id":"q","method":"x"}"#));
        let bid = backend(&sent)[0]["id"].clone();

        // when
        let out = m.backend_undecodable(Some(bid));
        let client = m.client_undecodable(1, None);

        // then
        let reply = &to_client(&out, 2)[0];
        assert_eq!(reply["id"], "q");
        assert_eq!(reply["error"]["code"], -32603);
        assert_eq!(m.pending_count(), 0);
        assert_eq!(to_client(&client, 1)[0]["error"]["code"], -32700);
        assert_eq!(to_client(&client, 1)[0]["id"], Value::Null);
    }

    #[test]
    fn should_acknowledge_the_proxy_shutdown_when_backend_answers() {
        // given
        let mut m = initialized_pair();
        let req = v(&m.shutdown_request());

        // when
        let out = m.from_backend(respond(&req, r#"{"jsonrpc":"2.0","result":null}"#));

        // then
        assert!(matches!(out.as_slice(), [Output::ShutdownAcknowledged]));
    }
}
