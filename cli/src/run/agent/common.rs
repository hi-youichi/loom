//! Shared stderr stream handling for terminal agent runs.

use loom::{MessageChunk, MessageChunkKind};

use super::{print_reply_timestamp, EventState};

fn print_stream_chunk(chunk: &MessageChunk) {
    if chunk.kind == MessageChunkKind::Thinking {
        eprint!("{}", chunk.content);
        let _ = std::io::Write::flush(&mut std::io::stderr());
    } else {
        print!("{}", chunk.content);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

pub(crate) fn handle_messages(
    s: &mut EventState,
    chunk: &MessageChunk,
    output_timestamp: bool,
) {
    if !s.reply_started {
        if let Some(ref ad) = s.agent_display {
            eprintln!("AGENT: {}", ad);
        }
        if output_timestamp {
            print_reply_timestamp();
        }
        s.reply_started = true;
    }
    print_stream_chunk(chunk);
}

pub(crate) fn usage_simple(s: &mut EventState, prompt_tokens: u32, completion_tokens: u32) {
    s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(prompt_tokens);
    s.total_completion_tokens = s.total_completion_tokens.saturating_add(completion_tokens);
    tracing::info!(
        prompt_tokens,
        completion_tokens,
        total_tokens = prompt_tokens + completion_tokens,
        "LLM usage"
    );
}

pub(crate) fn usage_react(
    s: &mut EventState,
    prompt_tokens: u32,
    completion_tokens: u32,
    prefill_duration: Option<std::time::Duration>,
    decode_duration: Option<std::time::Duration>,
) {
    s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(prompt_tokens);
    s.total_completion_tokens = s.total_completion_tokens.saturating_add(completion_tokens);

    match (prefill_duration, decode_duration) {
        (Some(prefill), Some(decode)) => {
            let prefill_secs = prefill.as_secs_f64();
            let decode_secs = decode.as_secs_f64();
            let total_secs = prefill_secs + decode_secs;
            let prefill_rate = if prefill_secs > 0.0 {
                prompt_tokens as f64 / prefill_secs
            } else {
                0.0
            };
            let decode_rate = if decode_secs > 0.0 {
                completion_tokens as f64 / decode_secs
            } else {
                0.0
            };
            eprintln!(
                "\nLLM: {:.2}s | prefill: {}t / {:.2}s = {:.0} t/s | decode: {}t / {:.2}s = {:.0} t/s",
                total_secs,
                prompt_tokens,
                prefill_secs,
                prefill_rate,
                completion_tokens,
                decode_secs,
                decode_rate
            );
        }
        _ => {
            eprintln!(
                "\nLLM: prompt={}, completion={}",
                prompt_tokens, completion_tokens
            );
        }
    }

    tracing::info!(
        prompt_tokens,
        completion_tokens,
        total_tokens = prompt_tokens + completion_tokens,
        "LLM usage"
    );
}
