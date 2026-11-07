//! Opt-in local WKWebView integration harness, excluded from normal builds.
use tauri::webview::PageLoadEvent;

pub fn attach(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.on_page_load(|webview, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        let Ok(path) = std::env::var("HGHUB_SMOKE_CONFIG") else {
            return;
        };
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(config) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return;
        };
        let code = if config.get("kind").and_then(|v| v.as_str()) == Some("download-settings") {
            format!(
                "window.__HG_SMOKE_CONFIG = {};\n{}",
                config,
                include_str!("../tests/download-settings-smoke.js")
            )
        } else {
            format!(
                "{}\nwindow.__HG_SMOKE_CONFIG = {};\n{}",
                include_str!("../../node_modules/hls.js/dist/hls.min.js"),
                config,
                include_str!("../tests/webview-smoke.js")
            )
        };
        if let Err(error) = webview.eval(code) {
            eprintln!("[smoke] WebView harness failed: {error}");
        }
    })
}
