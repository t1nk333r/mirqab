//! The closed list of error codes (architecture §5.2). A new code is a
//! protocol change.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Code {
    BadRequest,
    UnknownMethod,
    UnknownStream,
    Unavailable,
    Refused,
    NeedsConfirm,
    NeedsHelper,
    Failed,
}

impl Code {
    pub const ALL: [Code; 8] = [
        Code::BadRequest,
        Code::UnknownMethod,
        Code::UnknownStream,
        Code::Unavailable,
        Code::Refused,
        Code::NeedsConfirm,
        Code::NeedsHelper,
        Code::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Code::BadRequest => "bad-request",
            Code::UnknownMethod => "unknown-method",
            Code::UnknownStream => "unknown-stream",
            Code::Unavailable => "unavailable",
            Code::Refused => "refused",
            Code::NeedsConfirm => "needs-confirm",
            Code::NeedsHelper => "needs-helper",
            Code::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Code;

    #[test]
    fn the_list_is_closed_and_matches_the_architecture() {
        let names: Vec<String> = Code::ALL
            .iter()
            .map(|c| {
                serde_json::to_value(c)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "bad-request",
                "unknown-method",
                "unknown-stream",
                "unavailable",
                "refused",
                "needs-confirm",
                "needs-helper",
                "failed"
            ]
        );
        for c in Code::ALL {
            assert_eq!(serde_json::to_value(c).unwrap(), c.as_str());
        }
    }

    #[test]
    fn an_unknown_code_does_not_parse() {
        assert!(serde_json::from_str::<Code>("\"teapot\"").is_err());
    }
}
