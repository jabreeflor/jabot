//! Dispatch JSON-RPC requests to typed host handlers.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::protocol::error::RpcError;
use super::protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use super::protocol::methods::{
    HelloParams, InboxListParams, PermissionReplyParams, PromptParams, ResumeFromParams,
    SessionCancelParams, ThreadFoldParams, ThreadOpenParams, ThreadRefParams, HOST_HEALTH,
    HOST_HELLO, INBOX_LIST, PERMISSION_REPLY, SESSION_CANCEL, SESSION_PROMPT, SYNC_RESUME_FROM,
    THREAD_ARCHIVE, THREAD_DELETE, THREAD_FOLD, THREAD_OPEN, THREAD_REOPEN, THREAD_STATE,
};
use super::HostSession;

pub fn dispatch(session: &mut HostSession, request: JsonRpcRequest) -> JsonRpcResponse {
    if let Err(err) = request.validate() {
        return JsonRpcResponse::from_rpc_error(request.id, err);
    }

    match handle(session, &request) {
        Ok(result) => JsonRpcResponse::success(request.id, result),
        Err(err) => JsonRpcResponse::from_rpc_error(request.id, err),
    }
}

fn handle(session: &mut HostSession, request: &JsonRpcRequest) -> Result<Value, RpcError> {
    match request.method.as_str() {
        HOST_HELLO => {
            let params: HelloParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.hello(params)?)
        }
        HOST_HEALTH => to_value(session.health()),
        SESSION_PROMPT => {
            session.require_hello()?;
            let params: PromptParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.session_prompt(params)?)
        }
        SESSION_CANCEL => {
            session.require_hello()?;
            let params: SessionCancelParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.session_cancel(params)?)
        }
        PERMISSION_REPLY => {
            session.require_hello()?;
            let params: PermissionReplyParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.permission_reply(params)?)
        }
        THREAD_FOLD => {
            session.require_hello()?;
            let params: ThreadFoldParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_fold(params)?)
        }
        THREAD_OPEN => {
            session.require_hello()?;
            let params: ThreadOpenParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_open(params)?)
        }
        THREAD_REOPEN => {
            session.require_hello()?;
            let params: ThreadRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_reopen(params)?)
        }
        THREAD_ARCHIVE => {
            session.require_hello()?;
            let params: ThreadRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_archive(params)?)
        }
        THREAD_DELETE => {
            session.require_hello()?;
            let params: ThreadRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_delete(params)?)
        }
        THREAD_STATE => {
            session.require_hello()?;
            let params: ThreadRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_state(params)?)
        }
        INBOX_LIST => {
            session.require_hello()?;
            let params: InboxListParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.inbox_list(params)?)
        }
        SYNC_RESUME_FROM => {
            session.require_hello()?;
            let params: ResumeFromParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.resume_from(params))
        }
        _ => Err(RpcError::MethodNotFound),
    }
}

fn parse_params<T: DeserializeOwned>(params: Option<&Value>) -> Result<T, RpcError> {
    let value = params.cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| RpcError::InvalidParams(e.to_string()))
}

fn parse_params_or_default<T: DeserializeOwned + Default>(
    params: Option<&Value>,
) -> Result<T, RpcError> {
    match params {
        None => Ok(T::default()),
        Some(Value::Null) => Ok(T::default()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| RpcError::InvalidParams(e.to_string())),
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|e| RpcError::Internal(e.to_string()))
}
