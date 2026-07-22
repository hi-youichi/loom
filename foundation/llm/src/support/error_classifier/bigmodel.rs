//! BigModel (智谱) Error Classification Strategy
//!
//! BigModel API uses business error codes in response bodies.
//! Reference: https://docs.bigmodel.cn/cn/api/api-code

use super::{ApiErrorParser, HttpRetryPolicy, RetryDecision};

const BIGMODEL_RETRYABLE_CODES: &[&str] = &[
    "500", "1200", "1210", "1213", "1214", "1230", "1231", "1234", "1261", "1302", "1303", "1304",
    "1305", "1308", "1310", "1312", "1313",
];

#[cfg(test)]
const BIGMODEL_NON_RETRYABLE_CODES: &[&str] = &[
    "1000", "1001", "1002", "1003", "1004", "1110", "1111", "1112", "1113", "1120", "1121", "1211",
    "1212", "1215", "1220", "1221", "1222", "1300", "1301", "1309", "1311",
];

pub struct BigModelRetryPolicy;

impl HttpRetryPolicy for BigModelRetryPolicy {
    fn is_retryable_status(&self, status: u16, error_body: &str) -> bool {
        if matches!(status, 429 | 500..=504 | 524 | 598 | 599) {
            return true;
        }
        if matches!(status, 400 | 422) {
            let parser = BigModelApiParser;
            return parser.classify_api_error(error_body).is_retryable();
        }
        false
    }
}

pub struct BigModelApiParser;

impl BigModelApiParser {
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

impl ApiErrorParser for BigModelApiParser {
    fn extract_error_code(&self, message: &str) -> Option<String> {
        BigModelApiParser::parse_error_code(message)
    }

    fn is_retryable_code(&self, code: &str) -> bool {
        BIGMODEL_RETRYABLE_CODES.contains(&code)
    }

    fn classify_by_message_pattern(&self, message: &str) -> RetryDecision {
        let msg = message.to_lowercase();
        if msg.contains("参数非法")
            || msg.contains("并发")
            || msg.contains("频率")
            || msg.contains("流量限制")
            || msg.contains("访问量过大")
            || msg.contains("网络错误")
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
        let parser = BigModelApiParser;
        for code in BIGMODEL_RETRYABLE_CODES {
            assert!(
                parser.is_retryable_code(code),
                "Code {} should be retryable",
                code
            );
        }
    }

    #[test]
    fn non_retryable_error_codes() {
        let parser = BigModelApiParser;
        for code in BIGMODEL_NON_RETRYABLE_CODES {
            assert!(
                !parser.is_retryable_code(code),
                "Code {} should be non-retryable",
                code
            );
        }
    }

    #[test]
    fn extracts_error_code_from_message() {
        let parser = BigModelApiParser;
        assert_eq!(
            parser.extract_error_code("messages 参数非法 (code: 1214)"),
            Some("1214".to_string())
        );
        assert_eq!(
            parser.extract_error_code("error (code: 1002)"),
            Some("1002".to_string())
        );
        assert_eq!(parser.extract_error_code("no code here"), None);
    }

    #[test]
    fn code_1214_is_retryable() {
        let parser = BigModelApiParser;
        let message = "messages 参数非法。请检查文档。 (code: 1214)";
        assert_eq!(parser.classify_api_error(message), RetryDecision::Retryable);
    }

    #[test]
    fn chinese_rate_limit_is_retryable() {
        let parser = BigModelApiParser;
        assert_eq!(
            parser.classify_by_message_pattern("并发数过高"),
            RetryDecision::Retryable
        );
        assert_eq!(
            parser.classify_by_message_pattern("请求频率超限"),
            RetryDecision::Retryable
        );
    }

    #[test]
    fn auth_error_is_not_retryable() {
        let parser = BigModelApiParser;
        let message = "Authentication Token非法 (code: 1002)";
        assert_eq!(
            parser.classify_api_error(message),
            RetryDecision::NonRetryable
        );
    }

    #[test]
    fn bigmodel_http_policy_with_retryable_code() {
        let policy = BigModelRetryPolicy;
        assert!(policy.is_retryable_status(400, "messages 参数非法 (code: 1214)"));
    }

    #[test]
    fn bigmodel_http_policy_with_non_retryable_code() {
        let policy = BigModelRetryPolicy;
        assert!(!policy.is_retryable_status(400, "Authentication Token非法 (code: 1002)"));
    }

    #[test]
    fn bigmodel_http_policy_with_429() {
        let policy = BigModelRetryPolicy;
        assert!(policy.is_retryable_status(429, ""));
    }
}
