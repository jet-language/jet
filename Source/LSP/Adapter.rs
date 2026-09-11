//! Typed adapters for host extensions carried by the LSP JSON-RPC boundary.
//!
//! The LSP server owns the only frame/parser.  This module receives the
//! already-decoded `DataTree`, converts it once into the devserver request
//! types, and never calls the raw-JSON convenience API in `EditorHost`.



use jet_devserver::EditorHost::{
    EditorCommand, EditorCommandRequest, EditorHost, EditorHostError, EditorHostResponse,
    EditorHostTransport, EditorPanelSelectionRequest, EditorReconnectRequest,
    EditorSourceNavigationRequest, EditorWorkbenchOpenRequest, EDITOR_COMMAND_METHOD,
    EDITOR_NAVIGATE_METHOD, EDITOR_RECONNECT_METHOD, EDITOR_SELECT_METHOD,
    EDITOR_WORKBENCH_METHOD, EDITOR_MAX_HISTORY,
};
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::json_str;
fn object_get<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

/// One stateful transport is shared by VS Code and Zed requests.
///
/// The adapter intentionally stores no editor-specific state.  `EditorHost`
/// validates the host-neutral identity, stream cursor, panel, and command
/// grant on every typed request.
pub(crate) struct EditorHostAdapter {
    transport: Option<EditorHostTransport>,
}

impl EditorHostAdapter {
    pub(crate) fn new() -> Self {
        Self { transport: None }
    }

    /// Dispatch one already-parsed LSP extension request to the typed host.
    pub(crate) fn dispatch(
        &mut self,
        method: &str,
        params: Option<&DataTree>,
    ) -> Result<String, EditorHostError> {
        let params = params.ok_or_else(|| {
            EditorHostError::InvalidRequest("editor host request is missing `params`".to_string())
        })?;
        let object = params.as_object().map_err(|_| {
            EditorHostError::InvalidRequest("editor host request `params` must be an object".to_string())
        })?;
        let context = RequestContext::parse(object)?;
        self.sync_transport(&context)?;
        let transport = self.transport.as_mut().ok_or_else(|| {
            EditorHostError::InvalidRequest("editor host transport was not initialized".to_string())
        })?;

        let response = match method {
            EDITOR_WORKBENCH_METHOD => {
                EditorHostResponse::Workbench(transport.open(&parse_open(object, &context)?)?)
            }
            EDITOR_RECONNECT_METHOD => {
                EditorHostResponse::Workbench(transport.reconnect(&parse_reconnect(object, &context)?)?)
            }
            EDITOR_NAVIGATE_METHOD => {
                EditorHostResponse::Navigation(transport.navigate_source(&parse_navigation(object, &context)?)?)
            }
            EDITOR_SELECT_METHOD => {
                EditorHostResponse::Selection(transport.select_panel(&parse_selection(object, &context)?)?)
            }
            EDITOR_COMMAND_METHOD => {
                EditorHostResponse::Command(transport.command(&parse_command(object, &context)?)?)
            }
            _ => return Err(EditorHostError::UnsupportedMethod(method.to_string())),
        };
        Ok(response.serialize())
    }

    fn sync_transport(&mut self, context: &RequestContext) -> Result<(), EditorHostError> {
        if let Some(transport) = self.transport.as_mut() {
            let actual = context.identity()?;
            let expected = transport.identity();
            if actual.session_id != expected.session_id {
                return Err(EditorHostError::SessionMismatch {
                    expected: expected.session_id.clone(),
                    actual: actual.session_id,
                });
            }
            if actual.source_id != expected.source_id {
                return Err(EditorHostError::SourceMismatch {
                    expected: expected.source_id.clone(),
                    actual: actual.source_id,
                });
            }
            if actual.source_path != expected.source_path {
                return Err(EditorHostError::SourceMismatch {
                    expected: expected.source_path.clone(),
                    actual: actual.source_path,
                });
            }
            if actual.revision != expected.revision {
                return Err(EditorHostError::RevisionMismatch {
                    expected: expected.revision.clone(),
                    actual: actual.revision,
                });
            }
            if let Some(workbench_url) = &context.workbench_url {
                if workbench_url != transport.workbench_url() {
                    return Err(EditorHostError::InvalidRequest(
                        "workbench_url cannot change for an active editor session".to_string(),
                    ));
                }
            }
            if let Some(grants) = &context.grants {
                let expected = transport.grants().collect::<Vec<_>>();
                let mut actual = grants.iter().map(String::as_str).collect::<Vec<_>>();
                actual.sort_unstable();
                if expected != actual {
                    return Err(EditorHostError::InvalidRequest(
                        "command grants cannot change for an active editor session".to_string(),
                    ));
                }
            }
            return Ok(());
        }

        let workbench_url = context.workbench_url.clone().ok_or_else(|| {
            EditorHostError::InvalidRequest(
                "the first editor host request must provide `workbench_url`".to_string(),
            )
        })?;
        let transport = EditorHostTransport::new(
            workbench_url,
            context.session_id.clone(),
            context.source_id.clone(),
            context.source_path.clone(),
            context.revision.clone(),
            context.grants.clone().unwrap_or_default(),
        )?;
        self.transport = Some(transport);
        Ok(())
    }
}

/// Map a typed host failure to an LSP error code without turning it back into
/// an untyped result or executing a rejected action.
pub(crate) fn editor_host_error_code(error: &EditorHostError) -> i64 {
    match error {
        EditorHostError::SessionMismatch { .. }
        | EditorHostError::SourceMismatch { .. }
        | EditorHostError::RevisionMismatch { .. }
        | EditorHostError::CursorExpired { .. } => -32803,
        EditorHostError::CommandDenied { .. } | EditorHostError::LoopbackViolation(_) => -32090,
        EditorHostError::InvalidRequest(_)
        | EditorHostError::InvalidEnvelope(_)
        | EditorHostError::UnsupportedEditor(_)
        | EditorHostError::UnsupportedCommand(_)
        | EditorHostError::UnsupportedMethod(_)
        | EditorHostError::SourcePathRejected(_)
        | EditorHostError::PanelNotFound(_) => -32602,
    }
}

struct RequestContext {
    host: EditorHost,
    session_id: String,
    source_id: String,
    source_path: String,
    revision: String,
    workbench_url: Option<String>,
    grants: Option<Vec<String>>,
}

impl RequestContext {
    fn parse(object: &[(String, DataTree)]) -> Result<Self, EditorHostError> {
        Ok(Self {
            host: EditorHost::parse(&required_string(object, "editor")?)?,
            session_id: required_string(object, "session_id")?,
            source_id: required_string(object, "source_id")?,
            source_path: required_string(object, "source_path")?,
            revision: required_string(object, "revision")?,
            workbench_url: optional_string_alias(object, &["workbench_url", "workbenchUrl"])?,
            grants: parse_grants(object)?,
        })
    }

    fn identity(&self) -> Result<jet_devserver::EditorHost::EditorSessionIdentity, EditorHostError> {
        jet_devserver::EditorHost::EditorSessionIdentity::new(
            self.session_id.clone(),
            self.source_id.clone(),
            self.source_path.clone(),
            self.revision.clone(),
        )
    }
}

fn parse_open(
    object: &[(String, DataTree)],
    context: &RequestContext,
) -> Result<EditorWorkbenchOpenRequest, EditorHostError> {
    Ok(EditorWorkbenchOpenRequest {
        host: context.host,
        session_id: context.session_id.clone(),
        source_id: context.source_id.clone(),
        source_path: context.source_path.clone(),
        revision: context.revision.clone(),
        panel_id: optional_string(object, "panel_id")?,
        item_key: optional_string(object, "item_key")?,
        cursor: optional_u64(object, "cursor")?,
    })
}

fn parse_reconnect(
    object: &[(String, DataTree)],
    context: &RequestContext,
) -> Result<EditorReconnectRequest, EditorHostError> {
    Ok(EditorReconnectRequest {
        host: context.host,
        session_id: context.session_id.clone(),
        source_id: context.source_id.clone(),
        source_path: context.source_path.clone(),
        revision: context.revision.clone(),
        cursor: optional_u64(object, "cursor")?,
    })
}

fn parse_navigation(
    object: &[(String, DataTree)],
    context: &RequestContext,
) -> Result<EditorSourceNavigationRequest, EditorHostError> {
    Ok(EditorSourceNavigationRequest {
        host: context.host,
        session_id: context.session_id.clone(),
        source_id: context.source_id.clone(),
        source_path: context.source_path.clone(),
        revision: context.revision.clone(),
        line: required_u64(object, "line")?.try_into().map_err(|_| {
            EditorHostError::InvalidRequest("navigate.line exceeds u32".to_string())
        })?,
        column: required_u64(object, "column")?.try_into().map_err(|_| {
            EditorHostError::InvalidRequest("navigate.column exceeds u32".to_string())
        })?,
    })
}

fn parse_selection(
    object: &[(String, DataTree)],
    context: &RequestContext,
) -> Result<EditorPanelSelectionRequest, EditorHostError> {
    Ok(EditorPanelSelectionRequest {
        host: context.host,
        session_id: context.session_id.clone(),
        source_id: context.source_id.clone(),
        source_path: context.source_path.clone(),
        revision: context.revision.clone(),
        panel_id: required_string(object, "panel_id")?,
        item_key: optional_string(object, "item_key")?,
    })
}

fn parse_command(
    object: &[(String, DataTree)],
    context: &RequestContext,
) -> Result<EditorCommandRequest, EditorHostError> {
    let arguments = match object_get(object, "arguments") {
        None | Some(DataTree::Null) => Vec::new(),
        Some(DataTree::Array(values)) if values.len() <= EDITOR_MAX_HISTORY => values
            .iter()
            .map(|value| {
                json_str(value)
                    .map(str::to_string)
                    .ok_or_else(|| EditorHostError::InvalidRequest("command arguments must be strings".to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(DataTree::Array(_)) => {
            return Err(EditorHostError::InvalidRequest(
                "command arguments exceed the bounded limit".to_string(),
            ));
        }
        Some(_) => {
            return Err(EditorHostError::InvalidRequest(
                "command arguments must be an array or null".to_string(),
            ));
        }
    };
    Ok(EditorCommandRequest {
        host: context.host,
        session_id: context.session_id.clone(),
        source_id: context.source_id.clone(),
        source_path: context.source_path.clone(),
        revision: context.revision.clone(),
        command: EditorCommand::parse(&required_string(object, "command")?)?,
        arguments,
    })
}

fn required_string(object: &[(String, DataTree)], key: &str) -> Result<String, EditorHostError> {
    object_get(object, key)
        .and_then(json_str)
        .map(str::to_string)
        .ok_or_else(|| EditorHostError::InvalidRequest(format!("editor host field `{key}` must be a string")))
}

fn optional_string(
    object: &[(String, DataTree)],
    key: &str,
) -> Result<Option<String>, EditorHostError> {
    match object_get(object, key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(value) => json_str(value)
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| EditorHostError::InvalidRequest(format!("editor host field `{key}` must be a string or null"))),
    }
}

fn optional_string_alias(
    object: &[(String, DataTree)],
    keys: &[&str],
) -> Result<Option<String>, EditorHostError> {
    for key in keys {
        if object_get(object, key).is_some() {
            return optional_string(object, key);
        }
    }
    Ok(None)
}

fn required_u64(object: &[(String, DataTree)], key: &str) -> Result<u64, EditorHostError> {
    match object_get(object, key) {
        Some(DataTree::Int(value)) if *value >= 0 => Ok(*value as u64),
        _ => Err(EditorHostError::InvalidRequest(format!(
            "editor host field `{key}` must be a nonnegative integer"
        ))),
    }
}

fn optional_u64(
    object: &[(String, DataTree)],
    key: &str,
) -> Result<Option<u64>, EditorHostError> {
    match object_get(object, key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(DataTree::Int(value)) if *value >= 0 => Ok(Some(*value as u64)),
        _ => Err(EditorHostError::InvalidRequest(format!(
            "editor host field `{key}` must be a nonnegative integer or null"
        ))),
    }
}

fn parse_grants(object: &[(String, DataTree)]) -> Result<Option<Vec<String>>, EditorHostError> {
    let Some(value) = object_get(object, "grants") else {
        return Ok(None);
    };
    let DataTree::Array(values) = value else {
        if matches!(value, DataTree::Null) {
            return Ok(Some(Vec::new()));
        }
        return Err(EditorHostError::InvalidRequest(
            "editor host field `grants` must be an array or null".to_string(),
        ));
    };
    if values.len() > EDITOR_MAX_HISTORY {
        return Err(EditorHostError::InvalidRequest(
            "editor host grants exceed the bounded limit".to_string(),
        ));
    }
    values
        .iter()
        .map(|value| {
            json_str(value)
                .map(str::to_string)
                .ok_or_else(|| EditorHostError::InvalidRequest("editor host grants must be strings".to_string()))
        })
        .map(|grant| {
            grant.and_then(|grant| {
                if grant.is_empty() || grant.chars().any(char::is_control) {
                    Err(EditorHostError::InvalidRequest(
                        "editor host grants must be nonempty and free of control characters".to_string(),
                    ))
                } else {
                    Ok(grant)
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

