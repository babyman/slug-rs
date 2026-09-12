use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Vm;

use super::{Diagnostic, PROTOCOL_VERSION, Request, Response};

/// In-process owner of interactive-session protocol lifecycle.
pub struct Server {
    vm: Vm,
    initialized: bool,
    sessions: BTreeSet<String>,
    next_session: u64,
}

impl Server {
    /// Creates a server around the shared VM that future session execution will use.
    #[must_use]
    pub fn new(vm: Vm) -> Self {
        Self {
            vm,
            initialized: false,
            sessions: BTreeSet::new(),
            next_session: 0,
        }
    }

    /// Returns the shared embedded VM without starting session execution.
    #[must_use]
    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    /// Handles one decoded protocol request.
    #[must_use]
    pub fn handle(&mut self, request: Request) -> Response {
        match request.method.as_str() {
            "initialize" => self.initialize(request),
            "session.open" => self.open(request),
            "session.close" => self.close(request),
            "submit" => Response::failure(
                Some(request.id),
                request.session,
                Diagnostic::protocol(
                    "method_unavailable",
                    "method `submit` is not available during the server-shell milestone",
                ),
            ),
            method => Response::failure(
                Some(request.id),
                request.session,
                Diagnostic::protocol("unknown_method", format!("unknown method `{method}`")),
            ),
        }
    }

    /// Decodes and handles one NDJSON request line without terminating the server.
    #[must_use]
    pub fn handle_line(&mut self, line: &str) -> Response {
        match serde_json::from_str(line) {
            Ok(request) => self.handle(request),
            Err(error) => Response::failure(
                None,
                None,
                Diagnostic::protocol("malformed_json", format!("invalid request JSON: {error}")),
            ),
        }
    }

    fn initialize(&mut self, request: Request) -> Response {
        let params = match required_params::<InitializeParams>(&request, "initialize") {
            Ok(params) => params,
            Err(error) => return Response::failure(Some(request.id), None, *error),
        };
        if request.session.is_some() {
            return Response::failure(
                Some(request.id),
                request.session,
                Diagnostic::protocol("invalid_request", "`initialize` must not include a session"),
            );
        }
        if params.protocol != PROTOCOL_VERSION {
            return Response::failure(
                Some(request.id),
                None,
                Diagnostic::protocol(
                    "unsupported_protocol",
                    format!(
                        "protocol {} is unsupported; expected {PROTOCOL_VERSION}",
                        params.protocol
                    ),
                ),
            );
        }
        self.initialized = true;
        Response::success(
            request.id,
            None,
            json!({
                "protocol": PROTOCOL_VERSION,
                "capabilities": { "sessions": true }
            }),
        )
    }

    fn open(&mut self, request: Request) -> Response {
        if let Some(response) = self.require_initialized(&request) {
            return response;
        }
        if request.session.is_some() || request.params.is_some() {
            return Response::failure(
                Some(request.id),
                request.session,
                Diagnostic::protocol(
                    "invalid_request",
                    "`session.open` must not include a session or params",
                ),
            );
        }
        self.next_session += 1;
        let session = format!("s{}", self.next_session);
        self.sessions.insert(session.clone());
        Response::success(request.id, None, json!({ "session": session }))
    }

    fn close(&mut self, request: Request) -> Response {
        if let Some(response) = self.require_initialized(&request) {
            return response;
        }
        if request.params.is_some() {
            return Response::failure(
                Some(request.id),
                request.session,
                Diagnostic::protocol("invalid_request", "`session.close` does not accept params"),
            );
        }
        let Some(session) = request.session else {
            return Response::failure(
                Some(request.id),
                None,
                Diagnostic::protocol("missing_session", "`session.close` requires a session"),
            );
        };
        if !self.sessions.remove(&session) {
            return Response::failure(
                Some(request.id),
                Some(session.clone()),
                Diagnostic::protocol("unknown_session", format!("unknown session `{session}`")),
            );
        }
        Response::success(request.id, Some(session), Value::Null)
    }

    fn require_initialized(&self, request: &Request) -> Option<Response> {
        (!self.initialized).then(|| {
            Response::failure(
                Some(request.id),
                request.session.clone(),
                Diagnostic::protocol(
                    "not_initialized",
                    "send `initialize` before session requests",
                ),
            )
        })
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new(Vm::new())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitializeParams {
    protocol: u8,
}

fn required_params<T>(request: &Request, method: &str) -> Result<T, Box<Diagnostic>>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(params) = request.params.clone() else {
        return Err(Box::new(Diagnostic::protocol(
            "missing_params",
            format!("`{method}` requires params"),
        )));
    };
    serde_json::from_value(params).map_err(|error| {
        Box::new(Diagnostic::protocol(
            "invalid_params",
            format!("invalid `{method}` params: {error}"),
        ))
    })
}
