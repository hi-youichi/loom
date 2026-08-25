use skill::discovery::SkillRegistry;
use std::path::Path;

fn main() {
    println!("=== anureo home = {:?}", env_config::home::anureo_home());
    println!("=== USERPROFILE = {:?}", std::env::var("USERPROFILE").ok());

    let reg = SkillRegistry::discover(Path::new("."), &[]).expect("discover");
    let entries = reg.list();
    println!("=== discovered {} skills ===", entries.len());
    for e in entries {
        let cat = e.metadata.category.as_deref().unwrap_or("-");
        println!(
            "  [{:7}] {:32}  category={}",
            e.source.label(),
            e.metadata.name,
            cat
        );
        println!("      file: {}", e.skill_file.display());
    }
}
