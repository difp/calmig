#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

type ReplySender = mpsc::Sender<Result<Value, String>>;
struct Bridge {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, ReplySender>>>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Bridge {
    fn start() -> Result<Self, String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut child = Command::new("node")
            .arg(root.join("dist/quiet-bridge.js"))
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Не удалось запустить локальный адаптер".to_owned())?;
        let input = child.stdin.take().ok_or("Нет канала адаптера")?;
        let output = child.stdout.take().ok_or("Нет ответа адаптера")?;
        let pending: Arc<Mutex<HashMap<u64, ReplySender>>> = Arc::default();
        let listeners = Arc::clone(&pending);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if line.len() > 8_000_000 {
                    continue;
                }
                let Ok(response) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(id) = response.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                if let Ok(mut map) = listeners.lock() {
                    if let Some(sender) = map.remove(&id) {
                        let result = if response.get("ok").and_then(Value::as_bool) == Some(true) {
                            Ok(response.get("data").cloned().unwrap_or(Value::Null))
                        } else {
                            Err(
                                "Не удалось выполнить запрос. Проверьте уведомления Instagram."
                                    .to_owned(),
                            )
                        };
                        let _ = sender.send(result);
                    }
                }
            }
            if let Ok(mut map) = listeners.lock() {
                for (_, sender) in map.drain() {
                    let _ = sender.send(Err("Адаптер остановился".into()));
                }
            }
        });
        Ok(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            next_id: AtomicU64::new(1),
            pending,
        })
    }
    fn call(&self, mut request: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let object = request.as_object_mut().ok_or("Некорректный запрос")?;
        object.insert("id".into(), json!(id));
        let serialized = serde_json::to_string(&request).map_err(|_| "Некорректный запрос")?;
        if serialized.len() > 8192 {
            return Err("Слишком большой запрос".into());
        }
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "Ошибка адаптера")?
            .insert(id, sender);
        let written = self
            .input
            .lock()
            .map_err(|_| "Ошибка адаптера")
            .and_then(|mut input| {
                writeln!(input, "{serialized}").map_err(|_| "Адаптер недоступен")
            });
        if written.is_err() {
            if let Ok(mut p) = self.pending.lock() {
                p.remove(&id);
            }
            return Err("Адаптер недоступен".into());
        }
        match receiver.recv_timeout(Duration::from_secs(120)) {
            Ok(result) => result,
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&id);
                }
                Err("Время ожидания истекло".into())
            }
        }
    }
}
#[tauri::command]
async fn bridge(
    request: Value,
    window: WebviewWindow,
    state: State<'_, Arc<Bridge>>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("Недоступная команда".into());
    }
    let adapter = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || adapter.call(request))
        .await
        .map_err(|_| "Ошибка адаптера")?
}

fn allowed_login_url(url: &tauri::Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "www.instagram.com"
                    | "instagram.com"
                    | "accountscenter.instagram.com"
                    | "accountscenter.meta.com"
                    | "accountscenter.facebook.com"
                    | "www.facebook.com"
                    | "m.facebook.com"
            )
        )
}

fn browser_data_directory(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("web-login"))
        .map_err(|_| "Не удалось открыть данные входа".into())
}

fn show_login_window(app: &AppHandle, visible: bool, language: Option<&str>) -> Result<(), String> {
    let title = if language == Some("en") {
        "Sign in to Instagram"
    } else {
        "Вход в Instagram"
    };
    if let Some(window) = app.get_webview_window("instagram-login") {
        if visible {
            window
                .set_title(title)
                .map_err(|_| "Не удалось открыть окно входа")?;
            window.show().map_err(|_| "Не удалось открыть окно входа")?;
            window
                .set_focus()
                .map_err(|_| "Не удалось открыть окно входа")?;
        }
        return Ok(());
    }
    let directory = browser_data_directory(app)?;
    std::fs::create_dir_all(&directory).map_err(|_| "Не удалось открыть данные входа")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Не удалось защитить данные входа")?;
    }
    let url = "https://www.instagram.com/accounts/login/"
        .parse()
        .map_err(|_| "Некорректный адрес")?;
    WebviewWindowBuilder::new(app, "instagram-login", WebviewUrl::External(url))
        .title(title)
        .visible(visible)
        .inner_size(620.0, 760.0)
        .data_directory(directory)
        .on_navigation(allowed_login_url)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|_| "Не удалось открыть окно входа")?;
    Ok(())
}

#[tauri::command]
async fn open_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    language: Option<String>,
) -> Result<(), String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    show_login_window(&app, true, language.as_deref())
}

#[tauri::command]
async fn complete_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    state: State<'_, Arc<Bridge>>,
) -> Result<Value, String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    let window = app
        .get_webview_window("instagram-login")
        .ok_or("Сначала откройте страницу входа")?;
    let adapter = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let url = "https://www.instagram.com/"
            .parse()
            .map_err(|_| "Некорректный адрес")?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|_| "Не удалось получить сессию")?;
        let mut required = Vec::new();
        for name in ["sessionid", "ds_user_id", "csrftoken"] {
            let matching: Vec<_> = cookies
                .iter()
                .filter(|cookie| cookie.name() == name)
                .collect();
            if matching.len() != 1 {
                return Err("Войдите на странице Instagram и повторите".into());
            }
            required.push(json!({"name":name,"value":matching[0].value()}));
        }
        let result = adapter.call(json!({"action":"browser-login","cookies":required}))?;
        let _ = window.hide();
        Ok(result)
    })
    .await
    .map_err(|_| "Не удалось подключить аккаунт")?
}

#[tauri::command]
async fn restore_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    state: State<'_, Arc<Bridge>>,
    language: Option<String>,
) -> Result<Option<Value>, String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    if !browser_data_directory(&app)?.exists() {
        return Ok(None);
    }
    show_login_window(&app, false, language.as_deref())?;
    Ok(complete_instagram_login(app, caller, state).await.ok())
}

#[tauri::command]
async fn clear_instagram_login(app: AppHandle, caller: WebviewWindow) -> Result<(), String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    if let Some(window) = app.get_webview_window("instagram-login") {
        window
            .clear_all_browsing_data()
            .map_err(|_| "Не удалось удалить данные входа")?;
        let _ = window.hide();
    } else {
        let directory = browser_data_directory(&app)?;
        if directory.exists() {
            std::fs::remove_dir_all(directory).map_err(|_| "Не удалось удалить данные входа")?;
        }
    }
    Ok(())
}
fn main() {
    let adapter = match Bridge::start() {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    tauri::Builder::default()
        .manage(Arc::new(adapter))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "instagram-login" {
                    api.prevent_close();
                    let _ = window.hide();
                } else if window.label() == "main" {
                    if let Some(auth) = window.app_handle().get_webview_window("instagram-login") {
                        let _ = auth.destroy();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bridge,
            open_instagram_login,
            complete_instagram_login,
            restore_instagram_login,
            clear_instagram_login
        ])
        .run(tauri::generate_context!())
        .expect("Не удалось запустить приложение");
}

#[cfg(test)]
mod tests {
    use super::allowed_login_url;

    #[test]
    fn login_window_allows_only_official_https_hosts() {
        assert!(allowed_login_url(
            &"https://www.instagram.com/accounts/login/".parse().unwrap()
        ));
        assert!(allowed_login_url(
            &"https://accountscenter.meta.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://instagram.com.evil.example/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"http://www.instagram.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://user:pass@www.instagram.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://www.instagram.com:444/".parse().unwrap()
        ));
    }
}
