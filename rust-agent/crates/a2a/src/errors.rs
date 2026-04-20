use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AErrorResponse {
    pub error: A2AError,
}

impl A2AError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn task_not_found(task_id: &str) -> Self {
        Self::new("TaskNotFoundError", format!("Task {} not found", task_id))
    }

    pub fn task_not_cancelable(task_id: &str) -> Self {
        Self::new(
            "TaskNotCancelableError",
            format!(
                "Task {} is already in a terminal state and cannot be canceled",
                task_id
            ),
        )
    }

    pub fn unsupported_operation(msg: impl Into<String>) -> Self {
        Self::new("UnsupportedOperationError", msg)
    }

    pub fn unsupported_part_type(msg: impl Into<String>) -> Self {
        Self::new("ContentTypeNotSupportedError", msg)
    }

    pub fn version_not_supported(version: &str) -> Self {
        Self::new(
            "VersionNotSupportedError",
            format!("A2A version {} is not supported", version),
        )
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new("InvalidRequestError", msg)
    }

    pub fn push_notification_not_supported() -> Self {
        Self::new(
            "PushNotificationNotSupportedError",
            "Push notifications are not supported",
        )
    }
}

impl IntoResponse for A2AErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let status = match self.error.code.as_str() {
            "TaskNotFoundError" => StatusCode::NOT_FOUND,
            "TaskNotCancelableError" => StatusCode::CONFLICT,
            "UnsupportedOperationError" => StatusCode::METHOD_NOT_ALLOWED,
            "ContentTypeNotSupportedError" => StatusCode::BAD_REQUEST,
            "VersionNotSupportedError" => StatusCode::BAD_REQUEST,
            "InvalidRequestError" => StatusCode::BAD_REQUEST,
            "PushNotificationNotSupportedError" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}
