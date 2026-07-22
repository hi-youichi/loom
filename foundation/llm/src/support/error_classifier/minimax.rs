//! MiniMax Error Classification Strategy
//!
//! MiniMax API uses base_resp.status_code in response bodies.
//! Reference: https://platform.minimaxi.com/docs/api-reference/errorcode

use super::{ApiErrorParser, HttpRetryPolicy, RetryDecision};

const MINIMAX_RETRYABLE_CODES: &[&str] = &[
    "1000", "1001", "1002", "1024", "1033", "1041", "2045", "2056",
];

#[cfg(test)]
const MINIMAX_NON_RETRYABLE_CODES: &[&str] = &[
    "1004", "1008", "1026", "1027", "1039", "1042", "1043", "1044", "2013", "20132", "2037",
    "2038", "2039", "2042", "2048", "2049",
];

pub struct MiniMaxRetryPolicy;

impl HttpRetryPolicy for MiniMaxRetryPolicy {
    fn is_retryable_status(&self, status: u16, error_body: &str) -> bool {
        if matches!(status, 429 | 500..=504 | 524 | 598 | 599) {
            return true;
        }
        if matches!(status, 400 | 422) {
            let parser = MiniMaxApiParser;
            return parser.classify_api_error(error_body).is_retryable();
        }
        false
    }
}

pub struct MiniMaxApiParser;

impl MiniMaxApiParser {
    fn parse_error_code(message: &str) -> Option<String> {
        let lower = message.to_lowercase();
        if let Some(start) = lower.find("(code:") {
            let after = &message[start + 6..].trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
        None
    }
}

impl ApiErrorParser for MiniMaxApiParser {
    fn extract_error_code(&self, message: &str) -> Option<String> {
        MiniMaxApiParser::parse_error_code(message)
    }

    fn is_retryable_code(&self, code: &str) -> bool {
        MINIMAX_RETRYABLE_CODES.contains(&code)
    }

    fn classify_by_message_pattern(&self, message: &str) -> RetryDecision {
        let msg = message.to_lowercase();
        if msg.contains("请求超时")
            || msg.contains("请求频率超限")
            || msg.contains("内部错误")
            || msg.contains("系统错误")
            || msg.contains("连接数限制")
            || msg.contains("请求频率增长超限")
            || msg.contains("资源限制")
        {
            return RetryDecision::Retryable;
        }
        RetryDecision::NonRetryable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_error_codes() {
        let parser = MiniMaxApiParser;
        for code in MINIMAX_RETRYABLE_CODES {
            assert!(
                parser.is_retryable_code(code),
                "Code {} should be retryable",
                code
            );
        }
    }

    #[test]
    fn non_retryable_error_codes() {
        let parser = MiniMaxApiParser;
        for code in MINIMAX_NON_RETRYABLE_CODES {
            assert!(
                !parser.is_retryable_code(code),
                "Code {} should be non-retryable",
                code
            );
        }
    }

    #[test]
    fn extracts_error_code_from_message() {
        let parser = MiniMaxApiParser;
        assert_eq!(
            parser.extract_error_code("请求频率超限 (code: 1002)"),
            Some("1002".to_string())
        );
        assert_eq!(
            parser.extract_error_code("系统错误 (code: 1033)"),
            Some("1033".to_string())
        );
        assert_eq!(parser.extract_error_code("no code here"), None);
    }

    #[test]
    fn rate_limit_is_retryable() {
        let parser = MiniMaxApiParser;
        let message = "请求频率超限 (code: 1002)";
        assert_eq!(parser.classify_api_error(message), RetryDecision::Retryable);
    }

    #[test]
    fn system_error_is_retryable() {
        let parser = MiniMaxApiParser;
        let message = "系统错误 (code: 1033)";
        assert_eq!(parser.classify_api_error(message), RetryDecision::Retryable);
    }

    #[test]
    fn chinese_rate_limit_is_retryable() {
        let parser = MiniMaxApiParser;
        assert_eq!(
            parser.classify_by_message_pattern("请求频率超限，请稍后再试"),
            RetryDecision::Retryable
        );
        assert_eq!(
            parser.classify_by_message_pattern("请求超时"),
            RetryDecision::Retryable
        );
    }

    #[test]
    fn unauthorized_is_not_retryable() {
        let parser = MiniMaxApiParser;
        let message = "未授权 (code: 1004)";
        assert_eq!(
            parser.classify_api_error(message),
            RetryDecision::NonRetryable
        );
    }

    #[test]
    fn sensitive_content_is_not_retryable() {
        let parser = MiniMaxApiParser;
        let message = "输入内容涉敏 (code: 1026)";
        assert_eq!(
            parser.classify_api_error(message),
            RetryDecision::NonRetryable
        );
    }

    #[test]
    fn minimax_http_policy_with_retryable_code() {
        let policy = MiniMaxRetryPolicy;
        assert!(policy.is_retryable_status(400, "请求频率超限 (code: 1002)"));
    }

    #[test]
    fn minimax_http_policy_with_non_retryable_code() {
        let policy = MiniMaxRetryPolicy;
        assert!(!policy.is_retryable_status(400, "余额不足 (code: 1008)"));
    }

    #[test]
    fn minimax_http_policy_with_429() {
        let policy = MiniMaxRetryPolicy;
        assert!(policy.is_retryable_status(429, ""));
    }
}
