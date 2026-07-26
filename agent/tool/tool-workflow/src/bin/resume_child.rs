//! Subprocess helper for crash-and-resume integration tests.
//!
//! Usage:
//!   resume_child <base_dir> <script_name> <crash_after_n_calls>

use std::env;
use std::process::exit;

use luft::LuftBuilder;
use luft_core::testing::CrashBackend;

const SCRIPT_3PHASE: &str = r#"
function main()
  phase("collect")
  local r = agent({name = "a1", prompt = "prompt-1"})
  if not r.ok then report({error = "a1 failed"}) return end

  phase("analyze")
  local r = agent({name = "a2", prompt = "prompt-2"})
  if not r.ok then report({error = "a2 failed"}) return end

  phase("report")
  local r = agent({name = "a3", prompt = "prompt-3"})
  if not r.ok then report({error = "a3 failed"}) return end

  report({ok = true})
end
"#;

const SCRIPT_MULTI_AGENT: &str = r#"
function main()
  phase("research")
  agent({name = "a1", prompt = "prompt-1"})
  agent({name = "a2", prompt = "prompt-2"})
  agent({name = "a3", prompt = "prompt-3"})

  phase("write")
  agent({name = "a4", prompt = "prompt-4"})

  report({ok = true})
end
"#;

const SCRIPT_PARALLEL5: &str = r#"
function main()
  phase("gather")
  local urls = {"url1", "url2", "url3", "url4", "url5"}
  parallel(urls, function(url)
    return agent({name = "fetcher", prompt = "fetch " .. url})
  end)

  phase("summarize")
  agent({name = "summarizer", prompt = "summarize"})

  report({ok = true})
end
"#;

const SCRIPT_4PHASE: &str = r#"
function main()
  phase("a")
  agent({name = "a1", prompt = "prompt-a"})

  phase("b")
  agent({name = "b1", prompt = "prompt-b"})

  phase("c")
  agent({name = "c1", prompt = "prompt-c"})

  phase("d")
  agent({name = "d1", prompt = "prompt-d"})

  report({ok = true})
end
"#;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: resume_child <base_dir> <script_name> <crash_after_n_calls>");
        exit(2);
    }

    let base_dir = &args[1];
    let script_name = &args[2];
    let crash_after: u64 = args[3].parse().expect("crash_after as u64");

    let script = match script_name.as_str() {
        "3phase" => SCRIPT_3PHASE,
        "multi" => SCRIPT_MULTI_AGENT,
        "parallel5" => SCRIPT_PARALLEL5,
        "4phase" => SCRIPT_4PHASE,
        _ => {
            eprintln!("unknown script: {script_name}");
            exit(2);
        }
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let backend = CrashBackend::new(serde_json::json!("ok"), crash_after);

        let luft = LuftBuilder::new()
            .backend(backend)
            .base_dir(base_dir)
            .concurrency(1)
            .build()
            .expect("luft build");

        let handle = luft.start_script(script).await.expect("start_script");
        eprintln!("[child] run_dir: {}", handle.run_dir_name());

        let outcome = handle.join().await.expect("join");
        eprintln!(
            "[child] outcome: {:?}",
            outcome.result.as_ref().map(|v| v.to_string())
        );
    });
}
