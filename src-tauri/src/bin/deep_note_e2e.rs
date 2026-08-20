fn main() {
    if let Err(error) = mnemora_lib::deep_note_e2e::run_cli() {
        eprintln!("深度笔记 E2E 失败：{error}");
        std::process::exit(1);
    }
}
