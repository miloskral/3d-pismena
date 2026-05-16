// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bundle;
mod hardware_id;
mod keychain;
mod license;
mod trial;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// Embed the encrypted HTML bundle at compile time. The .enc file is placed
// here by encrypt-bundle.mjs during the build (see GitHub Actions workflow).
// We intentionally use include_bytes! (rather than reading from disk at runtime)
// so the plaintext HTML never lives on the user's filesystem.
const ENC_BUNDLE: &[u8] = include_bytes!("../resources/channel-letters.enc");
const LOGO_PNG: &[u8]   = include_bytes!("../resources/logo.png");

fn main() {
    // 1. Hardware UUID — needs to be computed once at startup and used in
    //    BOTH the SLP_INJECTED bridge AND the trial-server POST.
    let hw_uuid = hardware_id::machine_guid();

    // 2. Resolve trial state via the lumisign3d.com endpoint (fail-closed
    //    after 48 h of unreachability, matching macOS TrialService).
    let trial_snap = trial::resolve(&hw_uuid, APP_VERSION);
    let trial_js   = trial::to_js_literal(&trial_snap);

    // 3. Pull the persisted license key (if any) from the DPAPI-protected
    //    store.bin. JS-string-escape so it slots safely into the bridge.
    let store = keychain::load();
    let license_expr = match store.license.as_ref() {
        Some(key) => format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\"")),
        None      => "null".to_string(),
    };

    // 4. Decrypt the embedded .enc bundle into a UTF-8 HTML string. The plain
    //    HTML is then injected into the WebView2 via a custom URI scheme
    //    handler — never touches the user's disk.
    let html = match bundle::decrypt_html(ENC_BUNDLE, Some(LOGO_PNG)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[ChannelLetters] bundle decrypt failed: {e}");
            format!("<h1 style='font:18px sans-serif;color:#fff;background:#222;padding:20px'>Bundle decrypt failed — {e}</h1>")
        }
    };
    let html_arc: Arc<Vec<u8>> = Arc::new(html.into_bytes());

    // 5. The SLP_INJECTED bridge — must match the macOS WebViewHost's bridge
    //    exactly. Injected at document start via initialization_script.
    let init_script = format!(
        r#"window.SLP_INJECTED = {{
            license: {license_expr},
            hwUuid: "{hw}",
            host: 'win-native',
            trialServer: {trial_js}
        }};"#,
        license_expr = license_expr,
        hw = hw_uuid.replace('"', "\\\""),
        trial_js = trial_js,
    );

    // 6. STL-download bridge — analogous to the macOS WKScriptMessageHandler.
    //    Intercepts `<a download>` clicks on `blob:` URLs and forwards the
    //    bytes back to Rust via the Tauri event bus so we can pop a save
    //    dialog. (Implemented in window event handler below.)
    let dl_bridge = include_str!("download_bridge.js");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Custom URI scheme that serves the decrypted HTML in-memory.
        // WebView2 treats `https://channelletters.localhost` (the URL Tauri
        // generates from this scheme on Windows) as a secure context, so
        // crypto.subtle.verify and localStorage both work.
        .register_uri_scheme_protocol("channelletters", {
            let html_arc = html_arc.clone();
            move |_ctx, req| {
                use std::borrow::Cow;
                let path = req.uri().path();
                let body: Cow<'static, [u8]> = if path == "/" || path == "/index.html" || path.is_empty() {
                    Cow::Owned((*html_arc).clone())
                } else {
                    Cow::Borrowed(b"" as &'static [u8])
                };
                let status = if body.is_empty() { 404 } else { 200 };
                tauri::http::Response::builder()
                    .status(status)
                    .header("Content-Type", "text/html; charset=utf-8")
                    .header("Cache-Control", "no-store")
                    .body(body)
                    .unwrap()
            }
        })
        .setup(move |app| {
            // Build the application menu.  The "Copy this PC's Hardware ID"
            // item is the Windows mirror of the macOS App.swift entry — it
            // lets the buyer copy their MachineGuid to the clipboard before
            // emailing it to us for license generation.
            let hw_for_menu = hw_uuid.clone();

            let copy_hwid =
                MenuItem::with_id(app, "copy-hwid", "Copy this PC's Hardware ID", true, None::<&str>)?;
            let enter_license =
                MenuItem::with_id(app, "enter-license", "Enter License Key…", true, Some("CmdOrCtrl+Shift+L"))?;
            let about =
                MenuItem::with_id(app, "about", "About ChannelLetters", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = PredefinedMenuItem::quit(app, Some("Quit"))?;

            let app_menu = Submenu::with_items(
                app,
                "ChannelLetters",
                true,
                &[&about, &separator, &enter_license, &copy_hwid, &separator, &quit],
            )?;
            let menu = Menu::with_items(app, &[&app_menu])?;
            app.set_menu(menu)?;

            // Build the main window with our custom protocol.
            let url = tauri::WebviewUrl::CustomProtocol(
                "channelletters://localhost/index.html".parse().unwrap()
            );
            let win = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title("ChannelLetters")
                .inner_size(1200.0, 800.0)
                .min_inner_size(980.0, 640.0)
                .initialization_script(&init_script)
                .initialization_script(dl_bridge)
                .build()?;

            // Wire menu actions.
            let hw_clone = hw_for_menu.clone();
            let win_clone = win.clone();
            app.on_menu_event(move |app_handle, event| {
                match event.id().as_ref() {
                    "copy-hwid" => {
                        if let Err(e) = app_handle.clipboard().write_text(hw_clone.clone()) {
                            eprintln!("[ChannelLetters] clipboard write failed: {e}");
                        }
                    }
                    "enter-license" => {
                        // Forward to the HTML gate — same convention as macOS
                        // (LicenseStore.requestLicenseEntry).
                        let _ = win_clone.emit("slp:request-license-entry", ());
                    }
                    "about" => {
                        let _ = win_clone.emit("slp:show-about", ());
                    }
                    _ => {}
                }
            });

            // Receive STL download bytes from the JS bridge.
            let win_for_dl = win.clone();
            win.listen("slp:stl-download", move |event| {
                handle_stl_download(&win_for_dl, event.payload());
            });

            // Receive license activation attempts from the JS gate.
            let win_for_lic = win.clone();
            win.listen("slp:activate-license", move |event| {
                handle_activate_license(&win_for_lic, event.payload());
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// -------------------------------------------------------------------------
// Bridge handlers
// -------------------------------------------------------------------------

fn handle_stl_download(win: &tauri::WebviewWindow, payload: &str) {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use tauri_plugin_dialog::DialogExt;

    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { return };
    let filename = v.get("filename").and_then(|x| x.as_str()).unwrap_or("download.stl").to_string();
    let b64      = v.get("base64").and_then(|x| x.as_str()).unwrap_or("");
    let Ok(bytes) = STANDARD.decode(b64) else { return };

    let dialog = win.dialog().clone();
    dialog
        .file()
        .set_file_name(&filename)
        .add_filter("STL", &["stl"])
        .save_file(move |path| {
            if let Some(p) = path {
                let _ = std::fs::write(p.as_path().unwrap(), bytes);
            }
        });
}

fn handle_activate_license(win: &tauri::WebviewWindow, payload: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { return };
    let key = v.get("key").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let hw = hardware_id::machine_guid();

    let result = license::verify(&key, &hw);
    let reply = match result {
        license::Outcome::Ok(_) => {
            let mut store = keychain::load();
            store.license = Some(key.trim().to_string());
            let _ = keychain::save(&store);
            serde_json::json!({ "ok": true })
        }
        license::Outcome::Expired(_) => serde_json::json!({ "ok": false, "reason": "expired" }),
        license::Outcome::Invalid(r) => serde_json::json!({ "ok": false, "reason": r }),
    };
    let _ = win.emit("slp:license-result", reply);
}
