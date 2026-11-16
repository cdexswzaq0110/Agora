//! Headless Agora: the same server the desktop app embeds, for running without a window
//! (e.g. on an always-on PC you reach from your phone).
//!
//! ```text
//! agora-server [--port 4717] [--lan] [--demo] [--data <dir>]
//! ```

use agora_core::{Engine, Store};
use agora_server::{serve, spawn_background, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut port: Option<u16> = None;
    let mut lan = false;
    let mut data: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()),
            "--lan" => lan = true,
            "--demo" => std::env::set_var("AGORA_DEMO", "1"),
            "--data" => data = args.next(),
            "-h" | "--help" => {
                println!("agora-server [--port 4717] [--lan] [--demo] [--data <dir>]");
                return Ok(());
            }
            other => anyhow::bail!("未知參數：{other}（--help 查看用法）"),
        }
    }
    let store = Store::open(data.map(Into::into).unwrap_or_else(Store::default_root))?;
    let engine = Engine::new(store)?;
    if lan {
        engine.update_settings(|s| s.lan_enabled = true)?;
    }
    let port = port.unwrap_or(engine.settings().port);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let state = AppState::new(engine.clone(), port);
    let (code, _) = state.pairing.current();
    println!("Agora {} — http://127.0.0.1:{port}/", agora_core::VERSION);
    println!("資料夾：{}", engine.store().root().display());
    println!("手機配對碼：{code}（10 分鐘內有效，也可在網頁「設定」查看）");
    spawn_background(engine);
    serve(state, listener).await
}
