//! Dispatch JSON-RPC requests to typed host handlers.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::protocol::error::RpcError;
use super::protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use super::protocol::methods::{
    ChiefInvokeParams, CrewCreateParams, CrewRefParams, CrewUpdateParams, FolderRefParams,
    FolderRegisterParams, FolderUpdateParams, GithubStatusParams, HarnessDoctorParams, HelloParams,
    InboxListParams, PermissionPendingParams, PermissionReplyParams, PromptParams,
    ResumeFromParams, ScheduleCreateParams, ScheduleListParams, ScheduleRefParams,
    ScheduleUpdateParams, SessionCancelParams, ThreadFoldParams, ThreadOpenParams, ThreadRefParams,
    ThreadTranscriptParams, ToolRefParams, CHIEF_INVOKE, CHIEF_TOOLS, CREW_CREATE, CREW_LIST,
    CREW_REMOVE, CREW_THREAD, CREW_UPDATE, FOLDER_FORGET, FOLDER_LIST, FOLDER_REGISTER,
    FOLDER_UPDATE, GITHUB_STATUS, HARNESS_DOCTOR, HARNESS_LIST, HOST_HEALTH, HOST_HELLO,
    INBOX_LIST, PERMISSION_PENDING, PERMISSION_REPLY, SCHEDULE_CREATE, SCHEDULE_LIST,
    SCHEDULE_REMOVE, SCHEDULE_RUN, SCHEDULE_UPDATE, SESSION_CANCEL, SESSION_PROMPT,
    SUPERVISOR_STATUS, SYNC_RESUME_FROM, THREAD_ARCHIVE, THREAD_DELETE, THREAD_FOLD, THREAD_OPEN,
    THREAD_REOPEN, THREAD_RESUME, THREAD_STATE, THREAD_TRANSCRIPT, TOOLS_CONNECT, TOOLS_DISCONNECT,
    TOOLS_LIST,
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
        PERMISSION_PENDING => {
            session.require_hello()?;
            let params: PermissionPendingParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.permission_pending(params)?)
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
        THREAD_RESUME => {
            session.require_hello()?;
            let params: ThreadRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_resume(params)?)
        }
        SUPERVISOR_STATUS => {
            session.require_hello()?;
            to_value(session.supervisor_status()?)
        }
        THREAD_TRANSCRIPT => {
            session.require_hello()?;
            let params: ThreadTranscriptParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.thread_transcript(params)?)
        }
        INBOX_LIST => {
            session.require_hello()?;
            let params: InboxListParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.inbox_list(params)?)
        }
        HARNESS_LIST => {
            session.require_hello()?;
            to_value(session.harness_list()?)
        }
        HARNESS_DOCTOR => {
            session.require_hello()?;
            let params: HarnessDoctorParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.harness_doctor(params)?)
        }
        TOOLS_LIST => {
            session.require_hello()?;
            to_value(session.tools_list()?)
        }
        TOOLS_CONNECT => {
            session.require_hello()?;
            let params: ToolRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.tools_connect(params)?)
        }
        TOOLS_DISCONNECT => {
            session.require_hello()?;
            let params: ToolRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.tools_disconnect(params)?)
        }
        FOLDER_LIST => {
            session.require_hello()?;
            to_value(session.folder_list()?)
        }
        FOLDER_REGISTER => {
            session.require_hello()?;
            let params: FolderRegisterParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.folder_register(params)?)
        }
        FOLDER_UPDATE => {
            session.require_hello()?;
            let params: FolderUpdateParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.folder_update(params)?)
        }
        FOLDER_FORGET => {
            session.require_hello()?;
            let params: FolderRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.folder_forget(params)?)
        }
        CREW_LIST => {
            session.require_hello()?;
            to_value(session.crew_list()?)
        }
        CREW_CREATE => {
            session.require_hello()?;
            let params: CrewCreateParams = parse_params_or_default(request.params.as_ref())?;
            params.validate()?;
            to_value(session.crew_create(params)?)
        }
        CREW_UPDATE => {
            session.require_hello()?;
            let params: CrewUpdateParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.crew_update(params)?)
        }
        CREW_REMOVE => {
            session.require_hello()?;
            let params: CrewRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.crew_remove(params)?)
        }
        CREW_THREAD => {
            session.require_hello()?;
            let params: CrewRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.crew_thread(params)?)
        }
        SCHEDULE_LIST => {
            session.require_hello()?;
            let params: ScheduleListParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.schedule_list(params)?)
        }
        SCHEDULE_CREATE => {
            session.require_hello()?;
            let params: ScheduleCreateParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.schedule_create(params)?)
        }
        SCHEDULE_UPDATE => {
            session.require_hello()?;
            let params: ScheduleUpdateParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.schedule_update(params)?)
        }
        SCHEDULE_REMOVE => {
            session.require_hello()?;
            let params: ScheduleRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.schedule_remove(params)?)
        }
        SCHEDULE_RUN => {
            session.require_hello()?;
            let params: ScheduleRefParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.schedule_run(params)?)
        }
        CHIEF_TOOLS => {
            session.require_hello()?;
            to_value(session.chief_tools()?)
        }
        CHIEF_INVOKE => {
            session.require_hello()?;
            let params: ChiefInvokeParams = parse_params(request.params.as_ref())?;
            params.validate()?;
            to_value(session.chief_invoke(params)?)
        }
        GITHUB_STATUS => {
            session.require_hello()?;
            let params: GithubStatusParams = parse_params_or_default(request.params.as_ref())?;
            to_value(session.github_status(params)?)
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
