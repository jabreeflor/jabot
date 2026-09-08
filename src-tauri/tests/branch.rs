//! Conversation branching (#266) over the public host API.

use jabot_lib::host::{HostSession, JsonRpcRequest, RequestId, HOST_HELLO, THREAD_BRANCH};
use serde_json::json;

#[test]
fn hello_advertises_thread_branch() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = HostSession::load(dir.path());
    let hello = session
        .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
        .result
        .expect("hello");
    let methods = hello["methods"].as_array().unwrap();
    assert!(
        methods.iter().any(|m| m == THREAD_BRANCH),
        "host/hello should list {THREAD_BRANCH}: {methods:?}"
    );
    // A call without hello is refused the same way the other lifecycle
    // methods are — the method is real, not a leftover constant.
    let response = session.handle_request(JsonRpcRequest::new(
        RequestId::Number(2),
        THREAD_BRANCH,
        Some(json!({ "threadId": "t", "throughSeq": 1 })),
    ));
    // This session already said hello, so this is a not-found rather than
    // "no such method".
    assert!(response.error.is_some(), "{response:?}");
}
