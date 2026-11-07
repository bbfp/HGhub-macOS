mod api;
mod downloads;
mod media;
mod net;
mod network;
mod session;
#[cfg(feature = "macos-smoke")]
mod smoke;

use api::Api;
use tauri::Manager;

pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    if let Some(path) = std::env::var_os("HGHUB_DATA_DIR") {
        let path = std::path::PathBuf::from(path);
        if !path.is_absolute() {
            return Err("HGHUB_DATA_DIR 必须是完整路径".into());
        }
        return Ok(path);
    }
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "macos-smoke")]
    let builder = smoke::attach(builder);
    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(Api::new())
        .setup(|app| {
            if cfg!(target_os = "macos") { network::initialize(app.handle()); }
            network::bind_session_token(app.handle());
            if let Ok(resource_dir) = app.path().resource_dir() {
                let bundled = resource_dir.join("binaries/ffmpeg");
                if bundled.exists() {
                    std::env::set_var("HGHUB_FFMPEG", bundled);
                }
            }
            if let Ok(dir) = data_dir(app.handle()) {
                let _ = std::fs::create_dir_all(&dir);
                media::bind_cache(dir.join("cache/covers"));
                let state: tauri::State<Api> = app.state();
                state.session.bind(dir);
                let downloads = state.downloads.clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = downloads.recover(handle).await {
                        eprintln!("[downloads] {error}");
                    }
                });
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("hg", |app, request, responder| {
            let uri = request.uri().clone();
            let path = uri.path().to_string();
            let query = uri.query().unwrap_or("").to_string();
            let range = request
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let app = app.app_handle().clone();
            // 自定义协议在 Windows 上是 http://<scheme>.localhost，Android 上是 https://
            // 注意：请求 URI 的 scheme 是 "hg"，不能拿来拼子资源地址，否则 WebView 无法解析。
            let scheme = if cfg!(target_os = "macos") {
                "hg"
            } else {
                "http"
            }
            .to_string();
            tauri::async_runtime::spawn(async move {
                let resp =
                    media::handle(Some(&app), &path, &query, range.as_deref(), &scheme).await;
                responder.respond(resp);
            });
        })
        .invoke_handler(tauri::generate_handler![
            api::app_boot,
            api::debug_log,
            api::net_diag,
            network::network_settings,
            network::network_settings_set,
            network::network_settings_test,
            api::api_login,
            api::api_register,
            api::api_logout,
            api::api_me,
            api::api_feed,
            api::api_category,
            api::api_rank,
            api::api_search,
            api::cache_clear,
            downloads::download_settings,
            downloads::download_settings_set,
            downloads::download_limits_set,
            downloads::download_local_path,
            downloads::download_tasks,
            downloads::download_warnings,
            downloads::download_enqueue,
            downloads::download_pause,
            downloads::download_resume,
            downloads::download_cancel,
            api::api_posts,
            api::api_post,
            api::api_post_like,
            api::api_post_collect,
            api::api_post_comments,
            api::api_video,
            api::api_play,
            api::api_comments,
            api::api_comment_send,
            api::api_like,
            api::api_collect,
            api::api_follow,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
