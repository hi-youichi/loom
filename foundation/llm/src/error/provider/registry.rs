//! Provider parser registry.
//!
//! `parser_for(provider_id)` returns the parser for a models.dev provider.
//! 默认 OpenAI 兼容解析器覆盖 ~146 家；覆写覆盖业务码/特殊状态码/专有协议。

use model_spec_core::error::ProviderErrorParser;

use super::{
    AnthropicParser, AzureParser, BedrockParser, GoogleParser, LongCatParser, MiniMaxParser,
    MoonshotParser, OpenAiCompatParser, OpenRouterParser, StepFunParser, XiaomiParser, ZhipuParser,
};

/// Returns the error parser for the given models.dev provider id.
pub fn parser_for(provider_id: &str) -> Box<dyn ProviderErrorParser> {
    match provider_id {
        // OpenAI 协议 + 中国厂商业务码
        "zhipuai" | "zhipuai-coding-plan" | "zai" | "zai-coding-plan" => {
            Box::new(ZhipuParser::new(provider_id))
        }
        "xiaomi" | "xiaomi-token-plan-cn" | "xiaomi-token-plan-ams" | "xiaomi-token-plan-sgp" => {
            Box::new(XiaomiParser::new(provider_id))
        }
        "stepfun" | "stepfun-ai" | "stepfun-step-plan" | "stepfun-ai-step-plan" => {
            Box::new(StepFunParser::new(provider_id))
        }
        "moonshotai" | "moonshotai-cn" | "kimi-for-coding" => {
            Box::new(MoonshotParser::new(provider_id))
        }
        "longcat" => Box::new(LongCatParser::new(provider_id)),
        "openrouter" => Box::new(OpenRouterParser::new(provider_id)),
        // MiniMax 虽是 Anthropic 兼容端点，但有自有业务码
        "minimax" | "minimax-coding-plan" | "minimax-cn" | "minimax-cn-coding-plan" => {
            Box::new(MiniMaxParser::new(provider_id))
        }
        // Anthropic 协议
        "anthropic" | "thinkingmachines" | "freemodel" | "subconscious" => {
            Box::new(AnthropicParser::new(provider_id))
        }
        // Google / Vertex / Azure / Bedrock
        "google" | "google-vertex" | "google-vertex-anthropic" => {
            Box::new(GoogleParser::new(provider_id))
        }
        "azure" | "azure-cognitive-services" => Box::new(AzureParser::new(provider_id)),
        "amazon-bedrock" => Box::new(BedrockParser::new(provider_id)),
        _ => Box::new(OpenAiCompatParser::new(provider_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_spec_core::error::ErrorKind;

    #[test]
    fn dispatches_special_providers() {
        let cases: &[(&str, ErrorKind)] = &[
            ("zhipuai", ErrorKind::AuthFailed),    // 1003 → AuthFailed
            ("xiaomi", ErrorKind::ContentFilter),  // 421 → ContentFilter
            ("stepfun", ErrorKind::ContentFilter), // 451 → ContentFilter
            ("minimax", ErrorKind::Billing),       // 1008 → Billing
            ("anthropic", ErrorKind::Overloaded),  // 529 → Overloaded
            ("google", ErrorKind::RateLimited),    // RESOURCE_EXHAUSTED → RateLimited
            ("openrouter", ErrorKind::Billing),    // insufficient_quota → Billing
        ];
        for (provider, expect) in cases {
            let parser = parser_for(provider);
            let status = match (*provider, *expect) {
                ("xiaomi", _) => 421,
                ("stepfun", _) => 451,
                (_, ErrorKind::AuthFailed) => 401,
                (_, ErrorKind::ContentFilter) => 400,
                (_, ErrorKind::Billing) => 429,
                (_, ErrorKind::Overloaded) => 529,
                (_, ErrorKind::RateLimited) => 429,
                _ => 400,
            };
            let body = match *expect {
                ErrorKind::AuthFailed => br#"{"error":{"code":"1003"}}"#.as_slice(),
                ErrorKind::ContentFilter if *provider == "xiaomi" => br#"{}"#.as_slice(),
                ErrorKind::ContentFilter => br#"{}"#.as_slice(),
                ErrorKind::Billing if *provider == "minimax" => {
                    br#"{"error":{"code":"1008"}}"#.as_slice()
                }
                ErrorKind::Billing => {
                    br#"{"error":{"metadata":{"error_type":"insufficient_quota"}}}"#.as_slice()
                }
                ErrorKind::Overloaded => {
                    br#"{"type":"error","error":{"type":"overloaded_error"}}"#.as_slice()
                }
                ErrorKind::RateLimited => {
                    br#"{"error":{"status":"RESOURCE_EXHAUSTED"}}"#.as_slice()
                }
                _ => br#"{}"#.as_slice(),
            };
            let err = parser.parse(status, &[], body);
            assert_eq!(err.kind, *expect, "provider={provider}");
        }
    }

    #[test]
    fn unknown_provider_falls_back_to_openai() {
        let parser = parser_for("some-random-provider");
        let err = parser.parse(401, &[], br#"{"error":{"message":"bad"}}"#);
        assert_eq!(err.kind, ErrorKind::AuthFailed);
    }
}
