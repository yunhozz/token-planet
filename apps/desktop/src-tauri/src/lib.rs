pub mod collectors;
pub mod domain;
pub mod storage;

use std::{fs, io, sync::Mutex, time::Duration};

use tauri::{Emitter, Manager, State};

use collectors::discovery::{resolve_roots, scan_sources, RootOptions, ScanSummary, SourceConfig};
use storage::ledger::Ledger;

struct AppState {
    config: SourceConfig,
    ledger: Mutex<Ledger>,
    latest: Mutex<Option<ScanSummary>>,
}

impl AppState {
    fn scan(&self) -> Result<ScanSummary, String> {
        let mut ledger = self.ledger.lock().map_err(|_| "local ledger unavailable")?;
        let summary =
            scan_sources(&self.config, &mut ledger).map_err(|_| "usage scan unavailable")?;
        *self.latest.lock().map_err(|_| "usage status unavailable")? = Some(summary.clone());
        Ok(summary)
    }
}

#[tauri::command]
fn refresh_usage(state: State<'_, AppState>) -> Result<ScanSummary, String> {
    state.scan()
}

#[tauri::command]
fn current_usage(state: State<'_, AppState>) -> Result<Option<ScanSummary>, String> {
    Ok(state
        .latest
        .lock()
        .map_err(|_| "usage status unavailable")?
        .clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![refresh_usage, current_usage])
        .setup(|app| {
            let app_data = app.path().app_local_data_dir()?;
            fs::create_dir_all(&app_data)?;
            let ledger_path = app_data.join("usage-ledger.sqlite3");
            let timezone = match Ledger::saved_timezone(&ledger_path)? {
                Some(saved) => saved,
                None => iana_time_zone::get_timezone()
                    .map_err(|_| io::Error::other("system timezone unavailable"))?
                    .parse()
                    .map_err(|_| io::Error::other("system timezone is not an IANA timezone"))?,
            };
            let roots = RootOptions::from_env(None, None, timezone)
                .ok_or_else(|| io::Error::other("home directory unavailable"))?;
            let config = resolve_roots(&roots);
            let ledger = Ledger::open(&ledger_path, timezone)?;
            let state = AppState {
                config,
                ledger: Mutex::new(ledger),
                latest: Mutex::new(None),
            };
            let _ = state.scan();
            app.manage(state);
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(60));
                let state = handle.state::<AppState>();
                if let Ok(summary) = state.scan() {
                    let _ = handle.emit("usage-updated", summary);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
