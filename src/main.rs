#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use eframe::egui;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::Command;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// Lang
// ─────────────────────────────────────────────────────────────────────────────

struct Lang {
    title: &'static str,
    // connect screen
    connect_title: &'static str,
    connect_hint: &'static str,
    btn_connect: &'static str,
    status_not_connected: &'static str,
    status_connected_usb: &'static str,
    trust_hint: &'static str,
    // login screen
    login_title: &'static str,
    login_hint: &'static str,
    login_email: &'static str,
    login_password: &'static str,
    login_2fa: &'static str,
    login_2fa_hint: &'static str,
    btn_login: &'static str,
    btn_submit_code: &'static str,
    login_wrong: &'static str,
    // main screen
    dev_portal_link: &'static str,
    no_ipa: &'static str,
    btn_pick_ipa: &'static str,
    btn_install: &'static str,
    btn_demo_install: &'static str,
    renew_all_title: &'static str,
    btn_renew_all: &'static str,
    renew_all_busy_label: &'static str,
    btn_logout: &'static str,
    log_title: &'static str,
    status_working: &'static str,
    // guide screen (инструкция после успешной установки)
    guide_congrats_tmpl: &'static str,
    btn_guide_done: &'static str,
}

const RU: Lang = Lang {
    title: "AverStor",
    connect_title: "Подключение iPhone",
    connect_hint: "Подключите iPhone по USB и разблокируйте его.",
    btn_connect: "Сопрячь устройство",
    status_not_connected: "Устройство не найдено",
    status_connected_usb: "Устройство подключено, требуется сопряжение",
    trust_hint: "Нажмите «Доверять» на iPhone",
    login_title: "Вход в Apple ID",
    login_hint: "Данные отправляются только на серверы Apple для подписи приложений.",
    login_email: "Apple ID (email):",
    login_password: "Пароль:",
    login_2fa: "Код подтверждения:",
    login_2fa_hint: "Введите 6-значный код, который пришёл на ваше устройство",
    btn_login: "Войти",
    btn_submit_code: "Подтвердить",
    login_wrong: "Не удалось войти. Проверьте данные.",
    dev_portal_link: "Создать / проверить Apple Developer профиль →",
    no_ipa: "Выберите .ipa файл для установки",
    btn_pick_ipa: "Выбрать .ipa",
    btn_install: "Подписать и установить",
    btn_demo_install: "🧪 Демо (без Apple ID)",
    renew_all_title: "Продлите использование установленных приложений",
    btn_renew_all: "🔄 Продлить всё",
    renew_all_busy_label: "Продление...",
    btn_logout: "Выйти из аккаунта",
    log_title: "Журнал",
    status_working: "Выполняется...",
    guide_congrats_tmpl: "Поздравляем! Установка «{name}» завершена, чтобы начать им пользоваться осталось разрешить его использование на iPhone.",
    btn_guide_done: "Готово",
};

const EN: Lang = Lang {
    title: "AverStor",
    connect_title: "Connect iPhone",
    connect_hint: "Connect your iPhone via USB and unlock it.",
    btn_connect: "Pair device",
    status_not_connected: "No device found",
    status_connected_usb: "Device connected, pairing required",
    trust_hint: "Tap \"Trust\" on your iPhone",
    login_title: "Sign in to Apple ID",
    login_hint: "Your credentials are only sent to Apple's servers to sign apps.",
    login_email: "Apple ID (email):",
    login_password: "Password:",
    login_2fa: "Verification code:",
    login_2fa_hint: "Enter the 6-digit code sent to your device",
    btn_login: "Sign in",
    btn_submit_code: "Confirm",
    login_wrong: "Sign in failed. Check your credentials.",
    dev_portal_link: "Create / check your Apple Developer profile →",
    no_ipa: "Select an .ipa file to install",
    btn_pick_ipa: "Choose .ipa",
    btn_install: "Sign & install",
    btn_demo_install: "🧪 Demo (no Apple ID)",
    renew_all_title: "Renew your installed apps",
    btn_renew_all: "🔄 Renew all",
    renew_all_busy_label: "Renewing...",
    btn_logout: "Log out",
    log_title: "Log",
    status_working: "Working...",
    guide_congrats_tmpl: "Congratulations! Installation of \"{name}\" is complete — to start using it you just need to allow it on your iPhone.",
    btn_guide_done: "Done",
};

fn detect_lang() -> &'static Lang {
    for var in &["LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES"] {
        if let Ok(val) = std::env::var(var) {
            if val.to_lowercase().starts_with("ru") {
                return &RU;
            }
        }
    }
    &EN
}

// ─────────────────────────────────────────────────────────────────────────────
// paths / storage
// ─────────────────────────────────────────────────────────────────────────────

const SIDELOADER_DOWNLOAD_URL: &str =
    "https://github.com/Dadoum/Sideloader/releases/download/1.0-pre3/sideloader-cli-linux-x86_64.zip";

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn app_dir() -> PathBuf {
    home_dir().join("averstor")
}

fn sideloader_dir() -> PathBuf {
    app_dir().join("sideloader_app")
}

fn sideloader_bin() -> PathBuf {
    sideloader_dir().join("sideloader-cli-linux-x86_64")
}

fn creds_path() -> PathBuf {
    app_dir().join("creds.dat")
}

fn log_path() -> PathBuf {
    app_dir().join("averstor.log")
}

/// Открывает URL в браузере. Пробует xdg-open, затем популярные браузеры по очереди.
fn open_url(url: &str) {
    let browsers = ["xdg-open", "firefox", "chromium", "chromium-browser", "google-chrome", "brave-browser", "opera"];
    for b in browsers {
        if Command::new(b).arg(url).spawn().is_ok() {
            return;
        }
    }
}


/// отдельно от новой структуры. Скачивание бинарника, если его нигде нет.
fn ensure_sideloader_present(log: &Logger) -> bool {
    let _ = std::fs::create_dir_all(app_dir());

    let new_dir = sideloader_dir();
    let old_dir = home_dir().join("sideloader_app");

    if !new_dir.exists() && old_dir.exists() {
        log.push(format!("Найдена старая папка {}, переношу в {}", old_dir.display(), new_dir.display()));
        if let Err(e) = std::fs::rename(&old_dir, &new_dir) {
            log.push(format!("Не удалось перенести папку: {e}"));
        }
    }

    if sideloader_bin().exists() {
        return true;
    }

    log.push("Бинарник sideloader не найден, скачиваю...".into());
    let _ = std::fs::create_dir_all(&new_dir);

    let zip_path = new_dir.join("sideloader.zip");
    let status = Command::new("wget")
        .args(["-q", "-O", zip_path.to_str().unwrap_or_default(), SIDELOADER_DOWNLOAD_URL])
        .status();

    match status {
        Ok(s) if s.success() => {}
        _ => {
            log.push("Не удалось скачать sideloader (нет интернета или wget недоступен).".into());
            return false;
        }
    }

    log.push("Распаковка...".into());
    let unzip_ok = Command::new("unzip")
        .args(["-o", zip_path.to_str().unwrap_or_default(), "-d", new_dir.to_str().unwrap_or_default()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let _ = std::fs::remove_file(&zip_path);

    if !unzip_ok || !sideloader_bin().exists() {
        log.push("Распаковка не удалась или бинарник не найден после распаковки.".into());
        return false;
    }

    let _ = Command::new("chmod").args(["+x", sideloader_bin().to_str().unwrap_or_default()]).status();
    log.push("sideloader готов к работе.".into());
    true
}

// простая обфускация пароля перед записью на диск (не настоящая криптография)
fn obfuscate(s: &str) -> String {
    use base64::{engine::general_purpose, Engine as _};
    let xored: Vec<u8> = s.bytes().enumerate().map(|(i, b)| b ^ (0xA5u8.wrapping_add(i as u8))).collect();
    general_purpose::STANDARD.encode(xored)
}

fn deobfuscate(s: &str) -> Option<String> {
    use base64::{engine::general_purpose, Engine as _};
    let bytes = general_purpose::STANDARD.decode(s.trim()).ok()?;
    let plain: Vec<u8> = bytes.iter().enumerate().map(|(i, b)| b ^ (0xA5u8.wrapping_add(i as u8))).collect();
    String::from_utf8(plain).ok()
}

#[derive(Clone, Default)]
struct Creds {
    email: String,
    password: String,
}

fn load_creds() -> Option<Creds> {
    let content = std::fs::read_to_string(creds_path()).ok()?;
    let mut lines = content.lines();
    let email = lines.next()?.to_string();
    let pw_enc = lines.next()?.to_string();
    let password = deobfuscate(&pw_enc)?;
    Some(Creds { email, password })
}

fn save_creds(creds: &Creds) {
    let _ = std::fs::create_dir_all(app_dir());
    let content = format!("{}\n{}\n", creds.email, obfuscate(&creds.password));
    let _ = std::fs::write(creds_path(), content);
}

fn clear_creds() {
    let _ = std::fs::remove_file(creds_path());
}

fn ipas_dir() -> PathBuf {
    app_dir().join("ipas")
}

fn ipa_map_path() -> PathBuf {
    app_dir().join("ipa_map.dat")
}

/// Путь к файлу с временем последней успешной подписи (unix-секунды).
/// Используется чтобы показывать обратный отсчёт 7 дней без удаления App ID.
fn last_signed_path() -> PathBuf {
    app_dir().join("last_signed.dat")
}

fn save_last_signed_now() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = std::fs::create_dir_all(app_dir());
    let _ = std::fs::write(last_signed_path(), now.to_string());
}

fn load_last_signed() -> Option<i64> {
    std::fs::read_to_string(last_signed_path())
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
}

/// Карта bundle_id → путь к сохранённой копии .ipa в своей папке (~/averstor/ipas).
/// Нужна, чтобы кнопка "Продлить всё" не зависела от исходного файла, который
/// пользователь мог переместить/удалить — приложение хранит свою копию, и чтобы
/// при удалении приложения (корзина) можно было найти и стереть именно его .ipa.
fn load_ipa_map() -> Vec<(String, String)> {
    std::fs::read_to_string(ipa_map_path())
        .map(|c| {
            c.lines()
                .filter_map(|l| {
                    let mut parts = l.splitn(2, '|');
                    Some((parts.next()?.to_string(), parts.next()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn save_ipa_map(map: &[(String, String)]) {
    let _ = std::fs::create_dir_all(app_dir());
    let content: String = map.iter().map(|(b, p)| format!("{b}|{p}\n")).collect();
    let _ = std::fs::write(ipa_map_path(), content);
}

/// Считает, сколько записей карты реально указывают на существующий файл, и
/// сразу же убирает из карты те, что указывают на удалённые/перемещённые
/// файлы (например, пользователь вручную стёр .ipa из ~/averstor/ipas).
///
/// Раньше число на кнопке "Продлить всё (N)" бралось прямо из
/// `load_ipa_map().len()`, то есть считало ВСЕ когда-либо запомненные
/// записи, включая те, чей файл давно не существует — после удаления
/// .ipa пользователем число на кнопке не уменьшалось, пока не нажать
/// саму кнопку (только там была разовая самоочистка). Теперь очистка
/// происходит здесь же, при каждой перерисовке экрана, так что счётчик
/// всегда показывает реальное число файлов, которые в самом деле будут
/// переустановлены по нажатию.
fn count_existing_ipas_and_clean() -> usize {
    let map = load_ipa_map();
    let (valid, dead): (Vec<_>, Vec<_>) = map.into_iter().partition(|(_, p)| std::path::Path::new(p).exists());
    if !dead.is_empty() {
        save_ipa_map(&valid);
    }
    valid.len()
}

fn app_id_creations_path() -> PathBuf {
    app_dir().join("app_id_creations.dat")
}

/// Локальный журнал моментов (unix-секунды), когда эта программа создавала
/// НОВЫЙ App ID — то есть ставила bundle_id, которого раньше не было в
/// ipa_map.dat. Нужен, чтобы приближённо посчитать недельный лимит создания
/// App ID — Apple/sideloader-cli не отдают эту цифру напрямую, есть только
/// `app-id list`, который показывает текущие активные App ID, а не то,
/// сколько их было СОЗДАНО за последние 7 дней (удаление этот счётчик
/// у Apple не уменьшает).
///
/// ВАЖНО: это приближение. Если App ID создавались не через эту программу,
/// или это первый запуск после того, как лимит уже был частично израсходован
/// раньше, локальный счётчик не будет точно совпадать с тем, что видит Apple.
fn load_app_id_creations() -> Vec<i64> {
    std::fs::read_to_string(app_id_creations_path())
        .map(|c| c.lines().filter_map(|l| l.trim().parse::<i64>().ok()).collect())
        .unwrap_or_default()
}

fn save_app_id_creations(timestamps: &[i64]) {
    let _ = std::fs::create_dir_all(app_dir());
    let content: String = timestamps.iter().map(|t| format!("{t}\n")).collect();
    let _ = std::fs::write(app_id_creations_path(), content);
}

/// Записывает момент создания нового App ID и сразу подчищает записи старше
/// 7 дней — файл не растёт бесконечно, и при следующем чтении уже не нужно
/// фильтровать древние записи.
fn record_app_id_creation() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let mut list = load_app_id_creations();
    list.retain(|&t| now - t < 7 * 86400);
    list.push(now);
    save_app_id_creations(&list);
}

/// Сколько App ID было создано через эту программу за последние 7 дней
/// (скользящее окно от текущего момента, а не от понедельника).
fn app_id_creations_last_7_days() -> usize {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    load_app_id_creations()
        .into_iter()
        .filter(|&t| now - t < 7 * 86400)
        .count()
}

/// Сканирует ~/averstor/ipas/ и добавляет в карту любые .ipa-файлы, которые
/// туда были закинуты вручную (не через кнопку "Подписать и установить") и
/// поэтому отсутствуют в ipa_map.dat. Настоящий bundle_id для такого файла
/// неизвестен (его можно узнать только из вывода `sideloader install`, а не
/// просто прочитав файл), поэтому используем временный ключ на основе имени
/// файла с префиксом "file:" — этого достаточно, чтобы "Продлить всё" нашла
/// и переустановила сам файл. Если позже этот же файл будет реально
/// установлен через кнопку install, remember_ipa_for_bundle перезапишет
/// временную запись на настоящий bundle_id (старая по пути не дублируется,
/// см. ниже фильтр по path). Вызывается один раз при старте программы.
fn scan_ipas_dir_for_orphans(log: &Logger) {
    let dir = ipas_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };

    let mut map = load_ipa_map();
    let known_paths: std::collections::HashSet<String> =
        map.iter().map(|(_, p)| p.clone()).collect();

    let mut added = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("ipa")) != Some(true) {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        if known_paths.contains(&path_str) {
            continue;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "app".to_string());
        let pseudo_id = format!("file:{stem}");
        map.push((pseudo_id, path_str));
        added += 1;
    }

    if added > 0 {
        save_ipa_map(&map);
        log.push(format!(
            "Найдено {added} .ipa в папке averstor/ipas, добавлено в список для продления \
             (bundle id для них уточнится автоматически после первого продления).",
        ));
    }
}

/// Копирует выбранный пользователем .ipa в собственную папку приложения
/// (~/averstor/ipas/), чтобы дальше работать со своей копией, а не с
/// исходным файлом, который пользователь может переместить или удалить.
/// Возвращает путь к копии.
/// Копирует .ipa в свою папку (~/averstor/ipas/), избегая дублей.
///
/// Раньше при каждой повторной установке ТОГО ЖЕ исходного файла (например,
/// пользователь снова жмёт "Подписать и установить" на тот же Avito.ipa из
/// Downloads) функция видела, что в ipas/ уже лежит файл с таким именем, не
/// умела понять, что это та же самая программа, и создавала новый файл с
/// суффиксом _1, _2... Карта ipa_map.dat обновлялась на этот новый путь, а
/// старая копия оставалась лежать без записи в карте либо позже терялась —
/// в результате "Продлить всё" в какой-то момент пыталось переустановить
/// несуществующий путь и просто пропускало приложение, молча ничего не делая
/// на телефоне (см. лог: "Файл не найден, пропускаю").
///
/// Теперь "тот же файл" определяется по содержимому (хешу), а не только по
/// имени: если в ipas/ уже есть файл с именем как у источника, но другим
/// содержимым — это другая версия (новый .ipa с тем же именем), и она
/// записывается ПОД ТЕМ ЖЕ ИМЕНЕМ, перезаписывая старую копию, а не рядом
/// под новым. Это держит карту ipa_map.dat актуальной без накопления мёртвых
/// путей.
fn copy_ipa_to_store(original_path: &str) -> std::io::Result<String> {
    let dir = ipas_dir();
    std::fs::create_dir_all(&dir)?;
    let src = std::path::Path::new(original_path);
    let base = src.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "app.ipa".to_string());
    let dest = dir.join(&base);

    if dest == src {
        // Файл уже выбран прямо из своей папки хранения — копировать
        // никуда не нужно, это и есть та же копия.
        return Ok(dest.to_string_lossy().to_string());
    }

    if dest.exists() {
        // Сравниваем содержимое, чтобы не копировать заново, если файл
        // не менялся (быстрее на больших .ipa), но при отличии — перезаписать
        // под тем же именем, а не плодить _1/_2.
        let same = files_identical(&dest, src).unwrap_or(false);
        if !same {
            std::fs::copy(src, &dest)?;
        }
    } else {
        std::fs::copy(src, &dest)?;
    }

    Ok(dest.to_string_lossy().to_string())
}

/// Быстрое сравнение двух файлов: сначала по размеру (отбрасывает почти все
/// несовпадения мгновенно), затем по содержимому байт-в-байт только если
/// размеры совпали — для .ipa в десятки/сотни МБ это намного дешевле, чем
/// гонять полноценный криптографический хеш на каждую установку.
fn files_identical(a: &std::path::Path, b: &std::path::Path) -> std::io::Result<bool> {
    let meta_a = std::fs::metadata(a)?;
    let meta_b = std::fs::metadata(b)?;
    if meta_a.len() != meta_b.len() {
        return Ok(false);
    }
    use std::io::Read;
    let mut fa = std::fs::File::open(a)?;
    let mut fb = std::fs::File::open(b)?;
    let mut buf_a = [0u8; 65536];
    let mut buf_b = [0u8; 65536];
    loop {
        let na = fa.read(&mut buf_a)?;
        let nb = fb.read(&mut buf_b)?;
        if na != nb {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
        if buf_a[..na] != buf_b[..nb] {
            return Ok(false);
        }
    }
}

/// Проверяет, похож ли токен на reverse-DNS bundle id (минимум 2 точки,
/// только допустимые символы).
fn looks_like_bundle_id(s: &str) -> bool {
    !s.is_empty()
        && s.matches('.').count() >= 2
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Пытается вытащить bundle id из новых строк журнала, появившихся во время
/// установки. Поддерживает два формата вывода sideloader:
///
/// 1. Строка без кавычек (DEBUG-вывод при install):
///      `App IDs needed: com.example.app, com.example.app.Widget, ...`
///    Возвращает ПЕРВЫЙ (корневой) bundle id из списка — он всегда идёт
///    первым и не содержит суффиксов расширений.
///
/// 2. Обратные кавычки (как в `app-id list`):
///      `` - `com.example.app` for the app ... ``
///
/// Перед поиском применяем strip_ansi — PTY-вывод sideloader содержит ANSI
/// escape-коды и VT100 последовательности типа [K (erase-to-end-of-line),
/// которые прерывают строки в середине и сбивают поиск подстрок.
fn extract_bundle_from_lines(lines: &[String]) -> Option<String> {
    // Сначала ищем строку "App IDs needed:" — она содержит главный bundle id
    // первым в списке и появляется без обратных кавычек.
    for line in lines {
        let clean = strip_ansi(line).replace('\r', " ");
        if let Some(idx) = clean.find("App IDs needed:") {
            let after = clean[idx + "App IDs needed:".len()..].trim();
            // Берём первый токен через запятую — это корневой bundle id
            let first = after.split(',').next().unwrap_or("").trim();
            if looks_like_bundle_id(first) {
                return Some(first.to_string());
            }
        }
    }

    // Запасной вариант: ищем паттерн `bundle.id` в обратных кавычках
    for line in lines {
        let clean = strip_ansi(line);
        let mut rest = clean.as_str();
        while let Some(start) = rest.find('`') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('`') else { break };
            let candidate = after[..end].trim();
            if looks_like_bundle_id(candidate) {
                return Some(candidate.to_string());
            }
            rest = &after[end + 1..];
        }
    }

    None
}

/// Сохраняет/обновляет в карте путь к .ipa для конкретного bundle id.
fn remember_ipa_for_bundle(bundle_id: &str, stored_path: &str) {
    let mut map = load_ipa_map();
    // Убираем старые записи и по bundle_id, и по тому же пути — последнее
    // важно для файлов, которые scan_ipas_dir_for_orphans добавил с
    // временным псевдо-ключом "file:..." до того, как стал известен
    // настоящий bundle_id: иначе после первой реальной установки в карте
    // оставались бы СРАЗУ две записи на один и тот же файл (старая
    // псевдо-запись + новая настоящая), и "Продлить всё" переустанавливала
    // бы его дважды подряд.
    map.retain(|(b, p)| b != bundle_id && p != stored_path);
    map.push((bundle_id.to_string(), stored_path.to_string()));
    save_ipa_map(&map);
}

/// Удаляет из своей папки .ipa-файл(ы), связанные с любым из переданных
/// bundle id (главный App ID приложения + его расширения), и убирает их из
/// карты. Вызывается при нажатии на 🗑 у приложения.
fn forget_and_delete_ipas_for_bundles(bundle_ids: &[String]) {
    let mut map = load_ipa_map();
    let mut to_delete: Vec<String> = Vec::new();
    map.retain(|(b, p)| {
        if bundle_ids.contains(b) {
            to_delete.push(p.clone());
            false
        } else {
            true
        }
    });
    save_ipa_map(&map);
    for path in to_delete {
        let _ = std::fs::remove_file(path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// logger
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Logger {
    lines: Arc<Mutex<Vec<String>>>,
}

impl Logger {
    fn push(&self, line: String) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
            let _ = writeln!(f, "{line}");
        }
        self.lines.lock().unwrap().push(line);
    }

    fn snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// process helpers
// ─────────────────────────────────────────────────────────────────────────────

fn run_capture(cmd: &str, args: &[&str]) -> (bool, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr).to_string(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

fn sudo_capture(args: &[&str]) -> (bool, String) {
    // используем sudo без -S: предполагаем NOPASSWD либо что пользователь уже
    // авторизован в сессии (тикет sudo). Если потребуется пароль, sudo сам
    // запросит его в текущем TTY/полит-агенте.
    run_capture("sudo", args)
}

// ─────────────────────────────────────────────────────────────────────────────
// device connect
// ─────────────────────────────────────────────────────────────────────────────

fn connect_device(log: &Logger) -> bool {
    log.push("$ sudo pkill -9 usbmuxd".into());
    let (_, out1) = sudo_capture(&["pkill", "-9", "usbmuxd"]);
    if !out1.trim().is_empty() {
        log.push(out1.trim().to_string());
    }

    thread::sleep(Duration::from_millis(800));

    log.push("$ idevicepair pair".into());
    let (ok, out2) = run_capture("idevicepair", &["pair"]);
    log.push(out2.trim().to_string());

    ok && out2.to_uppercase().contains("SUCCESS")
}

// ─────────────────────────────────────────────────────────────────────────────
// sideloader interactive session (login / cert / install)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceStatus {
    NotConnected,
    ConnectedUsb,
    Paired,
}

/// Быстрая неблокирующая проверка состояния устройства без какого-либо
/// логирования (вызывается часто, в фоновом потоке поллинга).
fn check_device_status() -> DeviceStatus {
    let (_, out) = run_capture("idevice_id", &["-l"]);
    let has_device = out.lines().any(|l| !l.trim().is_empty());
    if !has_device {
        return DeviceStatus::NotConnected;
    }

    let (_, out2) = run_capture("idevicepair", &["validate"]);
    if out2.to_uppercase().contains("SUCCESS") {
        DeviceStatus::Paired
    } else {
        DeviceStatus::ConnectedUsb
    }
}

// ── FFI к C PTY-runner ───────────────────────────────────────────────────
extern "C" {
    fn pty_run(
        email: *const c_char,
        password: *const c_char,
        beacon_path: *const c_char,
        bin_path: *const c_char,
        args: *const *const c_char,
        line_cb: extern "C" fn(*const c_char, *mut c_void),
        ctx: *mut c_void,
    ) -> c_int;
}

/// Callback, который C-код вызывает на каждую строку вывода sideloader.
/// ctx — указатель на LineCallbackCtx.
extern "C" fn line_callback(line: *const c_char, ctx: *mut c_void) {
    let ctx = unsafe { &*(ctx as *const LineCallbackCtx) };
    let line_str = unsafe { CStr::from_ptr(line) }
        .to_string_lossy()
        .to_string();

    if line_str.trim() == "__AVERSTOR_WAIT_2FA__" {
        *ctx.waiting_2fa.lock().unwrap() = true;
        // Ждём код от GUI (кладётся в beacon-файл C-кодом напрямую,
        // здесь только выставляем флаг для UI).
        return;
    }

    // Сбрасываем флаг 2FA как только пришла следующая обычная строка
    {
        let mut w = ctx.waiting_2fa.lock().unwrap();
        if *w { *w = false; }
    }

    ctx.log.push(line_str);
}

struct LineCallbackCtx {
    log:         Logger,
    waiting_2fa: Arc<Mutex<bool>>,
}

/// Запускает sideloader через встроенный C PTY-runner.
///
/// Проблема: sideloader читает email через D-шный readln (stdin), но пароль
/// через POSIX getpass() — а getpass открывает /dev/tty напрямую, полностью
/// игнорируя stdin. Обычные Pipe-based подходы не работают для пароля.
///
/// Решение: C-код (pty_runner.c) создаёт псевдотерминал (PTY) и запускает
/// sideloader внутри него. С точки зрения sideloader он работает в настоящем
/// терминале, поэтому getpass() читает из slave-конца PTY.
/// C-код слинкован прямо в бинарь через build.rs + cc крейт.
fn run_sideloader_interactive(
    args: &[&str],
    email: &str,
    password: &str,
    code_rx: Arc<Mutex<Option<String>>>,
    waiting_2fa: Arc<Mutex<bool>>,
    log: &Logger,
) -> bool {
    let bin = sideloader_bin();
    log.push(format!("$ {} {}", bin.display(), args.join(" ")));

    // Уникальный путь beacon-файла для этого запуска
    let unique = format!(
        "2fa_beacon_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let beacon_path = app_dir().join(&unique);
    let _ = std::fs::remove_file(&beacon_path);

    // Строим NULL-terminated массив аргументов для execv
    let bin_cstr   = CString::new(bin.to_string_lossy().as_ref()).unwrap_or_default();
    let beacon_cstr = CString::new(beacon_path.to_string_lossy().as_ref()).unwrap_or_default();
    let email_cstr  = CString::new(email).unwrap_or_default();
    let passwd_cstr = CString::new(password).unwrap_or_default();

    // args[0] = bin_path (argv[0] для sideloader), затем аргументы, затем NULL
    let mut argv_cstrings: Vec<CString> = Vec::with_capacity(args.len() + 2);
    argv_cstrings.push(bin_cstr.clone());
    for a in args {
        argv_cstrings.push(CString::new(*a).unwrap_or_default());
    }
    let mut argv_ptrs: Vec<*const c_char> = argv_cstrings.iter().map(|s| s.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null()); // NULL-терминатор

    // Контекст для callback
    let cb_ctx = LineCallbackCtx {
        log:         log.clone(),
        waiting_2fa: Arc::clone(&waiting_2fa),
    };
    let ctx_ptr = &cb_ctx as *const LineCallbackCtx as *mut c_void;

    // Поток который передаёт 2FA-код в beacon-файл пока C-runner работает.
    // C-код опрашивает файл каждые 300мс сам, нам нужно только записать код.
    let beacon_path2 = beacon_path.clone();
    let code_rx2     = Arc::clone(&code_rx);
    let waiting_2fa2 = Arc::clone(&waiting_2fa);
    let _beacon_thread = thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(150));
            if *waiting_2fa2.lock().unwrap() {
                let mut guard = code_rx2.lock().unwrap();
                if let Some(code) = guard.take() {
                    drop(guard);
                    let _ = std::fs::write(&beacon_path2, &code);
                    break;
                }
            }
        }
    });

    // Вызываем C PTY-runner — блокирующий вызов, возвращает exit-код sideloader
    let exit_code = unsafe {
        pty_run(
            email_cstr.as_ptr(),
            passwd_cstr.as_ptr(),
            beacon_cstr.as_ptr(),
            bin_cstr.as_ptr(),
            argv_ptrs.as_ptr(),
            line_callback,
            ctx_ptr,
        )
    };

    let _ = std::fs::remove_file(&beacon_path);
    let ok = exit_code == 0;
    log.push(if ok {
        "[process exited OK]".into()
    } else {
        format!("[process exited with {}]", exit_code)
    });
    ok
}

#[allow(dead_code)]
fn full_log_contains(log: &Logger, needle: &str) -> bool {
    log.snapshot().iter().any(|l| l.to_lowercase().contains(needle))
}

/// Проверяет сертификаты Apple ID и отзывает существующий, чтобы `install`
/// гарантированно создал новый. Это обязательно для продления: если сертификат
/// не обновить, sideloader переиспользует старый и дата истечения не сдвинется.
///
/// Анализируем ТОЛЬКО строки текущего вызова (lines_before) — иначе старые
/// строки "currently registered certificates" из истории лога давали ложное
/// срабатывание и сертификат отзывался даже когда cert list вернул пустой список.
fn ensure_certificate(creds: &Creds, code_rx: Arc<Mutex<Option<String>>>, waiting_2fa: Arc<Mutex<bool>>, log: &Logger) -> bool {
    log.push("Проверяю сертификаты на Apple ID...".into());
    let lines_before = log.snapshot().len();
    let ok = run_sideloader_interactive(
        &["cert", "list", "-i"],
        &creds.email,
        &creds.password,
        Arc::clone(&code_rx),
        Arc::clone(&waiting_2fa),
        log,
    );

    if !ok {
        log.push("Не удалось проверить сертификаты (ошибка входа).".into());
        return false;
    }

    // Смотрим только на строки ЭТОГО вызова cert list — не весь лог.
    let snapshot = log.snapshot();
    let new_lines = &snapshot[lines_before.min(snapshot.len())..];
    let new_output_lower = new_lines.join("\n").to_lowercase();

    if new_output_lower.contains("you have 1 certificate") || new_output_lower.contains("currently registered certificates") || new_output_lower.contains("you have 1 certificates") {
        let cert_line = new_lines.iter().rev()
            .find(|l| l.contains('`') && l.to_lowercase().contains("serial"));
        if let Some(line) = cert_line {
            if let Some(serial) = extract_between(line, "serial number `", "`") {
                log.push(format!("Отзываю старый сертификат {serial} чтобы install создал новый..."));
                run_sideloader_interactive(
                    &["cert", "revoke", &serial, "-i"],
                    &creds.email,
                    &creds.password,
                    Arc::clone(&code_rx),
                    Arc::clone(&waiting_2fa),
                    log,
                );
            } else {
                // Сертификат есть, но серийный номер из вывода не извлёкся —
                // это значит формат вывода sideloader изменился. Логируем,
                // чтобы пользователь видел причину возможной ошибки 7460 дальше.
                log.push("Предупреждение: не удалось извлечь серийный номер сертификата из вывода sideloader. Продолжаю без отзыва — возможна ошибка 7460.".into());
            }
        } else {
            // cert list вернул признак наличия сертификата, но строки с серийником нет —
            // видимо, вывод без -d не содержит серийника. Попробуем с явным поиском.
            log.push("Не найдена строка с серийным номером сертификата. Попробуйте запустить с -d для отладки.".into());
        }
    } else {
        log.push("Активных сертификатов нет, install создаст новый автоматически.".into());
    }

    true
}

fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let i = s.find(start)? + start.len();
    let rest = &s[i..];
    let j = rest.find(end)?;
    Some(rest[..j].to_string())
}

/// Подписывает и устанавливает .ipa, обрабатывая повторную попытку при
/// конфликте сертификата (DeveloperPortalError 7460-подобные ситуации).
fn install_ipa(creds: &Creds, ipa_path: &str, code_rx: Arc<Mutex<Option<String>>>, waiting_2fa: Arc<Mutex<bool>>, log: &Logger) -> bool {
    log.push(format!("Устанавливаю {ipa_path} ..."));
    let lines_before = log.snapshot().len();
    let ok = run_sideloader_interactive(
        &["install", ipa_path, "-i", "-d"],
        &creds.email,
        &creds.password,
        Arc::clone(&code_rx),
        Arc::clone(&waiting_2fa),
        log,
    );

    if ok {
        return true;
    }

    // Проверяем только строки этого вызова install — не весь лог.
    let snapshot = log.snapshot();
    let new_lines = &snapshot[lines_before.min(snapshot.len())..];
    let new_output_lower = new_lines.join("\n").to_lowercase();

    if new_output_lower.contains("already have a current ios development certificate") {
        log.push("Сертификат конфликтует, отзываю старый и пробую снова...".into());
        if ensure_certificate(creds, Arc::clone(&code_rx), Arc::clone(&waiting_2fa), log) {
            return run_sideloader_interactive(
                &["install", ipa_path, "-i", "-d"],
                &creds.email,
                &creds.password,
                code_rx,
                waiting_2fa,
                log,
            );
        }
    }

    false
}

/// Загружает assets/logo.webp и регистрирует как текстуру egui.
/// Возвращает None, если файл отсутствует или не удалось декодировать —
/// в этом случае экран Connect просто не рисует лого.
fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../assets/logo.webp");
    let img = match image::load_from_memory(bytes) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            eprintln!("[averstor] logo.webp: не удалось декодировать: {e}");
            return None;
        }
    };
    let (w, h) = img.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture("averstor_logo", color_image, egui::TextureOptions::LINEAR))
}

/// Декодирует один байтовый слайс скриншота инструкции в текстуру egui.
fn decode_guide_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = match image::load_from_memory(bytes) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            eprintln!("[averstor] {name}: не удалось декодировать ({} байт): {e}", bytes.len());
            return None;
        }
    };
    let (w, h) = img.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
}

/// Загружает 5 скриншотов инструкции "как разрешить приложение на iPhone"
/// из assets/photo_1.jpg .. assets/photo_5.jpg. Элемент — None, если конкретный
/// файл отсутствует/не декодировался — тогда экран просто покажет плейсхолдер
/// вместо картинки для этого шага, а листание остальных шагов продолжит работать.
fn load_guide_textures(ctx: &egui::Context) -> Vec<Option<egui::TextureHandle>> {
    vec![
        decode_guide_texture(ctx, "averstor_guide_1", include_bytes!("../assets/photo_1.jpg")),
        decode_guide_texture(ctx, "averstor_guide_2", include_bytes!("../assets/photo_2.jpg")),
        decode_guide_texture(ctx, "averstor_guide_3", include_bytes!("../assets/photo_3.jpg")),
        decode_guide_texture(ctx, "averstor_guide_4", include_bytes!("../assets/photo_4.jpg")),
        decode_guide_texture(ctx, "averstor_guide_5", include_bytes!("../assets/photo_5.jpg")),
    ]
}

/// Выводит из имени файла .ipa человекочитаемое название приложения для
/// текста "Установка ... завершена": убирает расширение и, если совпадает
/// с обычным паттерном "Name_1.2.3.ipa" / "Name-1.2.3.ipa", отбрасывает
/// хвост с версией.
fn app_display_name_from_ipa_path(path: &str) -> String {
    let file_stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Отрезаем хвост вида "_1.2.3" / "-1.2.3", если он похож на версию.
    let cut_version = |s: &str, sep: char| -> Option<String> {
        let idx = s.rfind(sep)?;
        let tail = &s[idx + 1..];
        let looks_like_version = !tail.is_empty()
            && tail.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && tail.chars().all(|c| c.is_ascii_digit() || c == '.');
        if looks_like_version { Some(s[..idx].to_string()) } else { None }
    };

    cut_version(&file_stem, '_')
        .or_else(|| cut_version(&file_stem, '-'))
        .unwrap_or(file_stem)
}

// ─────────────────────────────────────────────────────────────────────────────
// theme
// ─────────────────────────────────────────────────────────────────────────────

fn setup_fonts_and_theme(cc: &eframe::CreationContext<'_>) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "terminus".into(),
        egui::FontData::from_static(include_bytes!("../assets/Terminus__TTF__500.ttf")),
    );
    fonts.font_data.insert(
        "terminus_bold".into(),
        egui::FontData::from_static(include_bytes!("../assets/Terminus__TTF__Bold_700.ttf")),
    );

    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "terminus".into());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "terminus".into());

    cc.egui_ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    let black = egui::Color32::BLACK;
    let white = egui::Color32::WHITE;
    let gray05 = egui::Color32::from_gray(12);
    let gray10 = egui::Color32::from_gray(22);
    let gray20 = egui::Color32::from_gray(45);

    visuals.override_text_color = Some(white);
    visuals.panel_fill = black;
    visuals.window_fill = black;
    visuals.extreme_bg_color = black;
    visuals.faint_bg_color = gray05;

    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.bg_fill = gray10;
        w.weak_bg_fill = gray05;
        w.fg_stroke = egui::Stroke::new(1.0, white);
        w.bg_stroke = egui::Stroke::new(1.0, gray20);
    }
    visuals.widgets.hovered.bg_fill = gray20;
    visuals.widgets.active.bg_fill = gray20;
    visuals.widgets.hovered.expansion = 0.0;
    visuals.widgets.active.expansion = 0.0;

    visuals.selection.bg_fill = egui::Color32::from_rgb(0, 80, 160);
    visuals.selection.stroke = egui::Stroke::new(1.0, white);
    visuals.striped = true;

    cc.egui_ctx.set_visuals(visuals);
}

/// Кнопка в стиле исходного проекта: цветная рамка/текст, при hover — заливка
/// цветом и чёрный текст.
fn colored_button(ui: &mut egui::Ui, label: &str, color: egui::Color32, w: f32, h: f32) -> egui::Response {
    let btn = ui.add_sized(
        [w, h],
        egui::Button::new(egui::RichText::new(label).size(14.0).color(color))
            .fill(egui::Color32::from_gray(22))
            .stroke(egui::Stroke::new(1.0, color)),
    );
    if btn.hovered() {
        let painter = ui.painter();
        painter.rect_filled(btn.rect, 2.0, color);
        painter.rect_stroke(btn.rect, 2.0, egui::Stroke::new(1.0, color));
        painter.text(
            btn.rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(14.0),
            egui::Color32::BLACK,
        );
    }
    btn
}

/// Кнопка с постоянной зелёной заливкой (не только на hover, в отличие
/// от colored_button) — используется для главного акцентного действия "Продлить
/// всё". Принимает `enabled` явным параметром, а не через ui.is_enabled() —
/// чтобы корректно перекрашиваться в серый независимо от того, в какой обёртке
/// (add_enabled_ui и т.п.) кнопка оказалась вызвана.
fn big_green_button(ui: &mut egui::Ui, label: &str, enabled: bool, w: f32, h: f32) -> egui::Response {
    let green = egui::Color32::from_rgb(80, 210, 80);
    let green_hover = egui::Color32::from_rgb(100, 230, 100);
    let fill = if enabled { green } else { egui::Color32::from_gray(50) };
    let text_color = if enabled { egui::Color32::BLACK } else { egui::Color32::from_gray(140) };

    let btn = ui.add_sized(
        [w, h],
        egui::Button::new(egui::RichText::new(label).size(14.0).strong().color(text_color))
            .fill(fill)
            .stroke(egui::Stroke::new(0.0, fill))
            .sense(if enabled { egui::Sense::click() } else { egui::Sense::hover() }),
    );
    if enabled && btn.hovered() {
        ui.painter().rect_filled(btn.rect, 4.0, green_hover);
        ui.painter().text(
            btn.rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(14.0),
            egui::Color32::BLACK,
        );
    }
    btn
}

// ─────────────────────────────────────────────────────────────────────────────
// Limits data
// ─────────────────────────────────────────────────────────────────────────────

/// Одно установленное приложение с датой истечения профиля.
#[derive(Clone)]
struct AppEntry {
    #[allow(dead_code)]
    name: String,
    bundle_id: String,
    /// Секунд до истечения (None = не удалось распарсить). Храним именно
    /// секунды, а не округлённые дни — иначе при отображении "дни и часы"
    /// дробная часть суток терялась бы на этом же шаге.
    seconds_left: Option<i64>,
}

/// Снимок лимитов бесплатного аккаунта Apple Developer.
#[derive(Clone, Default)]
struct Limits {
    /// Зарегистрированные App ID (макс 10)
    app_ids_used: usize,
    /// Установленные приложения с датами истечения
    apps: Vec<AppEntry>,
    /// Идёт ли опрос прямо сейчас
    loading: bool,
    /// Текст последней ошибки опроса
    error: Option<String>,
    /// Было ли хотя бы одно УСПЕШНОЕ получение данных. Отличает первую
    /// загрузку (когда показывать нечего, кроме спиннера на весь блок) от
    /// фонового обновления раз в 5 минут — во втором случае старые данные
    /// нужно оставлять на экране, а не прятать их за спиннером, иначе
    /// список приложений "мигал" и пропадал на каждое обновление.
    has_data: bool,
}



#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Connect,
    Login,
    Main,
    /// Инструкция из 5 листаемых скринов, показывается сразу после
    /// успешной установки .ipa — объясняет, как разрешить приложение
    /// от недоверенного разработчика в Настройках iPhone.
    Guide,
}

struct App {
    lang: &'static Lang,
    screen: Screen,
    log: Logger,
    logo: Option<egui::TextureHandle>,

    // connect screen
    device_status: Arc<Mutex<DeviceStatus>>,
    poll_started: bool,
    pairing: Arc<Mutex<bool>>,
    pair_result: Arc<Mutex<Option<bool>>>,
    /// Когда была последняя АВТОМАТИЧЕСКАЯ попытка сопряжения — как только
    /// комп видит iPhone по USB, но тот ещё не доверяет этому компу, кнопки
    /// нет: попытка запускается сама и повторяется, пока пользователь не
    /// нажмёт "Доверять" на самом iPhone.
    last_pair_attempt: Option<std::time::Instant>,

    // login screen
    email: String,
    password: String,
    code_input: String,
    login_busy: Arc<Mutex<bool>>,
    login_result: Arc<Mutex<Option<bool>>>,
    waiting_2fa: Arc<Mutex<bool>>,
    code_tx: Arc<Mutex<Option<String>>>,
    login_error: Option<String>,

    // main screen
    creds: Option<Creds>,
    ipa_path: Option<String>,
    install_busy: Arc<Mutex<bool>>,
    install_result: Arc<Mutex<Option<bool>>>,

    // limits panel
    limits: Arc<Mutex<Limits>>,
    limits_poll_started: bool,
    /// Выставляется в true сразу после успешной установки/продления, чтобы
    /// фоновый опрос лимитов (start_limits_poll) не ждал свой обычный
    /// 5-минутный интервал, а перепроверил `app-id list` сразу же — иначе
    /// пользователь видел старые данные в "Лимитах аккаунта" ещё несколько
    /// минут после успешного продления и не мог сразу убедиться в результате.
    force_limits_refresh: Arc<Mutex<bool>>,
    /// Идёт ли сейчас удаление (revoke) какого-то приложения из списка лимитов.
    revoke_busy: Arc<Mutex<bool>>,
    /// Идёт ли сейчас массовое "Продлить всё" по всей истории .ipa.
    renew_all_busy: Arc<Mutex<bool>>,
    /// Индекс цвета аватарки для каждого bundle_id (0..=8, циклически).
    avatar_colors: HashMap<String, usize>,

    // guide screen (инструкция после успешной установки)
    /// Текстуры 5 скринов инструкции, загружаются один раз при старте.
    guide_textures: Vec<Option<egui::TextureHandle>>,
    /// Текущий листаемый шаг инструкции, 0..=4.
    guide_index: usize,
    /// Имя устанавливаемого приложения для текста поздравления,
    /// определяется из имени файла .ipa в момент успешной установки.
    guide_app_name: String,
}

/// Вырезает ANSI escape-последовательности (цвета терминала) из строки —
/// PTY-вывод sideloader-а иногда содержит их прямо внутри строки со списком
/// App ID, что ломало парсинг даты истечения.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ ... <letter>  — пропускаем до завершающей буквы.
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&c2) = chars.peek() {
                    chars.next();
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Парсит строки вывода `sideloader app-id list`:
///   ` - `com.example.app` for the app `MyApp`, expiring on 2025-07-01.`
/// и возвращает список AppEntry.
fn parse_app_id_list(output: &str) -> Vec<AppEntry> {
    let mut apps = Vec::new();
    for raw_line in output.lines() {
        // PTY-вывод может содержать ANSI escape-коды (цвета) — вырезаем их,
        // иначе они могут попасть прямо в середину даты и сломать парсинг.
        let line = strip_ansi(raw_line);
        // Формат: " - `<bundle>` for the app `<name>`, expiring on <date>."
        let Some(after_dash) = line.trim().strip_prefix("- `") else { continue };
        let Some(bundle_end) = after_dash.find('`') else { continue };
        let bundle_id = after_dash[..bundle_end].to_string();
        let rest = &after_dash[bundle_end + 1..];
        // извлекаем имя
        let name = if let Some(n) = rest.find("` for the app `") {
            // имя идёт после "` for the app `"
            let name_start = &rest[n + "` for the app `".len()..];
            name_start.find('`').map(|e| name_start[..e].to_string()).unwrap_or_default()
        } else {
            bundle_id.clone()
        };
        // извлекаем дату "expiring on YYYY-Mon-DD" (D печатает Date.toString()
        // как "2025-Jun-26", а не "2025-06-26"). Берём ТОЛЬКО первое "слово"
        // после "expiring on " (до пробела), а не всё до конца строки — иначе
        // любой текст после даты (доп. примечания, скобки и т.п.) ломал split('-')
        // и seconds_left всегда получался None → отсюда сплошные "?".
        let seconds_left = rest.to_lowercase().find("expiring on ").and_then(|i| {
            // Сохраняем оригинальный регистр — month_from_abbrev ожидает "Jul", не "jul"
            let after = rest[i + "expiring on ".len()..].trim_start();
            // Формат sideloader: "2026-Jul-03 01:45:51." — берём первые два токена
            let mut tokens = after.split_whitespace();
            let date_str = tokens
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_ascii_alphanumeric());
            // Второй токен — время "HH:MM:SS" (может заканчиваться точкой/запятой)
            let time_str = tokens
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_ascii_digit() && c != ':');

            let parts: Vec<&str> = date_str.split('-').collect();
            if parts.len() == 3 {
                let y: i64 = parts[0].parse().ok()?;
                let m: i64 = match parts[1].parse::<i64>() {
                    Ok(v) => v,
                    Err(_) => month_from_abbrev(parts[1])?,
                };
                let d: i64 = parts[2].parse().ok()?;
                // Точный расчёт в секундах (без внешних крейтов) — дни и часы
                // в UI получаются делением этого значения, а не округлением
                // на этом шаге, иначе "дни и часы" были бы неточными.
                let mut expire_epoch = naive_date_to_epoch(y, m, d);
                // Добавляем компонент времени HH:MM:SS, если он есть —
                // без этого дата считалась как полночь UTC того дня, и
                // приложение после переподписи (которое истекает в ~01:45 UTC)
                // показывало «6д 17ч» вместо «7д 1ч».
                let time_parts: Vec<&str> = time_str.split(':').collect();
                if time_parts.len() == 3 {
                    if let (Ok(hh), Ok(mm), Ok(ss)) = (
                        time_parts[0].parse::<i64>(),
                        time_parts[1].parse::<i64>(),
                        time_parts[2].parse::<i64>(),
                    ) {
                        expire_epoch += hh * 3600 + mm * 60 + ss;
                    }
                }
                let now_epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                Some(expire_epoch - now_epoch)
            } else {
                None
            }
        });
        apps.push(AppEntry { name, bundle_id, seconds_left });
    }
    apps
}

/// Трёхбуквенные месяцы, как их печатает `Date.toString()` в D (рантайм sideloader-а).
fn month_from_abbrev(s: &str) -> Option<i64> {
    Some(match s {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4,
        "May" => 5, "Jun" => 6, "Jul" => 7, "Aug" => 8,
        "Sep" => 9, "Oct" => 10, "Nov" => 11, "Dec" => 12,
        _ => return None,
    })
}

/// Упрощённый перевод даты YYYY-MM-DD → Unix timestamp (без внешних крейтов).
fn naive_date_to_epoch(y: i64, m: i64, d: i64) -> i64 {
    // Алгоритм Зеллера / Julian Day → Unix
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + 12 * a - 3;
    let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045;
    (jdn - 2440588) * 86400 // 2440588 = JDN для 1970-01-01
}

/// Форматирует секунды до истечения как "Nд Mч" (или просто "Mч", если меньше
/// суток) — точнее, чем округление до целых дней, которое раньше показывало
/// "0д" весь последний день вне зависимости от того, истекает сертификат
/// через 23 часа или через 10 минут.
fn format_days_hours(seconds_left: i64) -> String {
    let secs = seconds_left.max(0);
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    if days > 0 {
        format!("{days}д {hours}ч")
    } else if hours > 0 {
        format!("{hours}ч")
    } else {
        let mins = (secs % 3600) / 60;
        format!("{mins}мин")
    }
}

/// Одно реальное приложение, "склеенное" из главного App ID и его расширений
/// (виджеты, intents, notification service и т.п. — у каждого свой App ID,
/// но пользователю это один установленный продукт).
#[derive(Clone)]
struct AppGroup {
    /// Человеко-читаемое имя, выведенное из bundle id (sideloader не хранит
    /// настоящее CFBundleDisplayName, только идентификатор).
    name: String,
    root_bundle_id: String,
    /// Минимум seconds_left среди главного App ID и всех его расширений —
    /// как только истекает любой из них, всё приложение требует переподписи.
    seconds_left: Option<i64>,
    /// Сколько расширений (помимо главного App ID) входит в группу.
    extra_count: usize,
    /// Все bundle id, входящие в группу (главный + расширения) — нужно,
    /// чтобы при удалении приложения отозвать все его App ID, а не только корневой.
    all_bundle_ids: Vec<String>,
}

/// Группирует "плоский" список App ID (где у каждого расширения — свой
/// отдельный bundle id вида `<root>.<Extension>`) в реальные приложения.
fn group_apps(entries: &[AppEntry]) -> Vec<AppGroup> {
    let mut sorted: Vec<&AppEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.bundle_id.len());

    let mut roots: Vec<String> = Vec::new();
    let mut groups: Vec<AppGroup> = Vec::new();

    for entry in &sorted {
        // Ищем самый длинный уже известный root, под который попадает этот bundle id.
        let parent = roots
            .iter()
            .filter(|r| entry.bundle_id.starts_with(r.as_str()) && entry.bundle_id.len() > r.len()
                && entry.bundle_id.as_bytes().get(r.len()) == Some(&b'.'))
            .max_by_key(|r| r.len())
            .cloned();

        match parent {
            Some(root) => {
                if let Some(g) = groups.iter_mut().find(|g| g.root_bundle_id == root) {
                    g.extra_count += 1;
                    g.seconds_left = min_seconds(g.seconds_left, entry.seconds_left);
                    g.all_bundle_ids.push(entry.bundle_id.clone());
                }
            }
            None => {
                roots.push(entry.bundle_id.clone());
                groups.push(AppGroup {
                    name: pretty_name_from_bundle(&entry.bundle_id),
                    root_bundle_id: entry.bundle_id.clone(),
                    seconds_left: entry.seconds_left,
                    extra_count: 0,
                    all_bundle_ids: vec![entry.bundle_id.clone()],
                });
            }
        }
    }

    groups
}

fn min_seconds(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// Sideloader не хранит настоящее имя приложения — `app-id list` отдаёт
/// в качестве "имени" сам bundle id. Вытаскиваем из reverse-DNS то, что
/// обычно является названием продукта (второй компонент: ru.avito.app →
/// avito, com.SideStore.SideStore → SideStore), и красиво капитализируем.
fn pretty_name_from_bundle(bundle_id: &str) -> String {
    let parts: Vec<&str> = bundle_id.split('.').collect();
    let raw = parts.get(1).copied().unwrap_or(bundle_id);
    let mut chars = raw.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => bundle_id.to_string(),
    }
}

/// Стабильный цвет аватарки по имени — чтобы одно и то же приложение всегда
/// выглядело одинаково между перезапусками.
/// 9 цветов аватарок, переключаются кликом
const AVATAR_PALETTE: [(u8, u8, u8); 9] = [
    (220,  60,  60),  // 0 красный
    (230, 140,  30),  // 1 оранжевый
    (220, 200,  40),  // 2 жёлтый
    ( 80, 200,  80),  // 3 зелёный
    ( 60, 190, 210),  // 4 голубой
    ( 70, 110, 230),  // 5 синий
    (150,  80, 220),  // 6 фиолетовый
    (230, 100, 180),  // 7 розовый
    (240, 240, 240),  // 8 белый
];

fn avatar_color_by_index(idx: usize) -> egui::Color32 {
    let (r, g, b) = AVATAR_PALETTE[idx % AVATAR_PALETTE.len()];
    egui::Color32::from_rgb(r, g, b)
}

fn default_avatar_index(seed: &str) -> usize {
    let hash = seed.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    (hash as usize) % AVATAR_PALETTE.len()
}

/// Рисует кликабельную круглую аватарку. Клик переключает цвет по кругу.
/// Возвращает true если пользователь кликнул (нужно обновить хранилище цветов).
fn draw_app_header_clickable(
    ui: &mut egui::Ui,
    name: &str,
    _seed: &str,
    color_idx: usize,
) -> bool {
    let color = avatar_color_by_index(color_idx);
    let diameter = 22.0_f32;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(diameter, diameter),
        egui::Sense::click(),
    );
    let painter = ui.painter();

    // подсветка при наведении
    let draw_color = if resp.hovered() {
        egui::Color32::from_rgb(
            (color.r() as u16).saturating_add(30).min(255) as u8,
            (color.g() as u16).saturating_add(30).min(255) as u8,
            (color.b() as u16).saturating_add(30).min(255) as u8,
        )
    } else {
        color
    };

    painter.circle_filled(rect.center(), diameter / 2.0, draw_color);

    // тёмная буква на светлых цветах (жёлтый, белый), белая на тёмных
    let brightness = color.r() as u16 + color.g() as u16 + color.b() as u16;
    let letter_color = if brightness > 500 {
        egui::Color32::from_gray(30)
    } else {
        egui::Color32::WHITE
    };

    let letter = name.chars().next().unwrap_or('?').to_uppercase().to_string();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::monospace(13.0),
        letter_color,
    );

    if resp.hovered() {
        resp.clone().on_hover_text("Кликни чтобы сменить цвет");
    }

    resp.clicked()
}

impl App {
    /// Отзывает (удаляет) все App ID, входящие в группу приложения —
    /// главный + все его расширения — через `sideloader app-id delete`.
    /// После этого освобождается слот в лимите "Приложения: x/3".
    fn revoke_app_group(&mut self, bundle_ids: Vec<String>) {
        if *self.revoke_busy.lock().unwrap() {
            return;
        }
        let Some(creds) = self.creds.clone() else { return };
        *self.revoke_busy.lock().unwrap() = true;

        let revoke_busy = Arc::clone(&self.revoke_busy);
        let limits = Arc::clone(&self.limits);
        let code_rx = Arc::clone(&self.code_tx);
        let waiting_2fa = Arc::clone(&self.waiting_2fa);
        let log = self.log.clone();

        thread::spawn(move || {
            for bundle_id in &bundle_ids {
                log.push(format!("Удаляю App ID {bundle_id} ..."));
                run_sideloader_interactive(
                    &["app-id", "delete", bundle_id, "-i"],
                    &creds.email,
                    &creds.password,
                    Arc::clone(&code_rx),
                    Arc::clone(&waiting_2fa),
                    &log,
                );
            }
            // Убираем удалённые bundle id из текущего снимка лимитов сразу,
            // не дожидаясь следующего опроса (раз в 5 минут) — но только из
            // списка приложений ("Приложения: x/3"), а не из app_ids_used.
            // "App IDs: x/10" — это лимит Apple на СОЗДАНИЕ App ID за неделю,
            // он не уменьшается при удалении (сам sideloader-cli прямо говорит
            // об этом в своей справке: "Delete an App ID (it won't let you
            // create more App IDs though)"), поэтому искусственно вычитать
            // отсюда нельзя — это создавало неверное, заниженное значение,
            // которое расходилось с тем, что Apple реально отдаёт.
            {
                let mut l = limits.lock().unwrap();
                l.apps.retain(|a| !bundle_ids.contains(&a.bundle_id));
            }
            // Удаляем .ipa этого приложения из своей папки (~/averstor/ipas/)
            // и убираем запись из карты bundle_id → путь — иначе файл остался
            // бы лежать на диске, а "Продлить всё" продолжало бы пытаться
            // переустановить уже отозванное приложение.
            forget_and_delete_ipas_for_bundles(&bundle_ids);
            log.push("Файлы .ipa удалённого приложения убраны из папки averstor.".into());
            *revoke_busy.lock().unwrap() = false;
        });
    }

    /// Запускает фоновый поток поллинга лимитов (вызывается один раз при входе на Main).
    fn start_limits_poll(&mut self) {
        if self.limits_poll_started {
            return;
        }
        self.limits_poll_started = true;
        let Some(creds) = self.creds.clone() else { return };
        let limits = Arc::clone(&self.limits);
        let code_rx = Arc::clone(&self.code_tx);
        let waiting_2fa = Arc::clone(&self.waiting_2fa);
        let log = self.log.clone();
        let force_refresh = Arc::clone(&self.force_limits_refresh);

        thread::spawn(move || {
            loop {
                {
                    let mut l = limits.lock().unwrap();
                    l.loading = true;
                    l.error = None;
                }

                // Запускаем `sideloader app-id list -i` через тот же PTY-механизм,
                // что и login/install — если Apple вдруг запросит 2FA прямо во время
                // фонового опроса, waiting_2fa выставится и пользователь увидит то же
                // самое поле ввода кода на главном экране (code_rx уйдёт туда же).
                let lines_before = log.snapshot().len();
                let ok = run_sideloader_interactive(
                    &["app-id", "list", "-i"],
                    &creds.email,
                    &creds.password,
                    Arc::clone(&code_rx),
                    Arc::clone(&waiting_2fa),
                    &log,
                );

                if !ok {
                    let mut l = limits.lock().unwrap();
                    l.loading = false;
                    l.error = Some("Не удалось получить лимиты (ошибка sideloader).".into());
                } else {
                    let snapshot = log.snapshot();
                    let start = lines_before.min(snapshot.len());
                    let output = snapshot[start..].join("\n");

                    let apps = parse_app_id_list(&output);
                    let app_ids_used = apps.len();

                    let mut l = limits.lock().unwrap();
                    l.apps = apps;
                    l.app_ids_used = app_ids_used;
                    l.loading = false;
                    l.has_data = true;
                }

                // Обычный интервал — раз в 5 минут, но проверяем
                // force_refresh каждую секунду внутри этого ожидания: если
                // start_install/start_renew_all только что завершились
                // успешно и выставили флаг, перепроверяем лимиты сразу же,
                // а не ждём оставшееся время до полных 5 минут.
                for _ in 0..300 {
                    thread::sleep(Duration::from_secs(1));
                    let mut f = force_refresh.lock().unwrap();
                    if *f {
                        *f = false;
                        break;
                    }
                }
            }
        });
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts_and_theme(cc);
        let logo = load_logo_texture(&cc.egui_ctx);
        let guide_textures = load_guide_textures(&cc.egui_ctx);
        let log = Logger::default();
        ensure_sideloader_present(&log);
        // Подхватываем .ipa, которые лежат в averstor/ipas, но были закинуты
        // туда не через эту программу (вручную) — без этого "Продлить всё"
        // не знала бы о их существовании, хотя файлы физически на месте.
        scan_ipas_dir_for_orphans(&log);

        let creds = load_creds();
        let email_prefill = creds.as_ref().map(|c| c.email.clone()).unwrap_or_default();
        let screen = Screen::Connect;

        Self {
            lang: detect_lang(),
            screen,
            log,
            logo,
            device_status: Arc::new(Mutex::new(DeviceStatus::NotConnected)),
            poll_started: false,
            pairing: Arc::new(Mutex::new(false)),
            pair_result: Arc::new(Mutex::new(None)),
            last_pair_attempt: None,
            email: email_prefill,
            password: String::new(),
            code_input: String::new(),
            login_busy: Arc::new(Mutex::new(false)),
            login_result: Arc::new(Mutex::new(None)),
            waiting_2fa: Arc::new(Mutex::new(false)),
            code_tx: Arc::new(Mutex::new(None)),
            login_error: None,
            creds,
            ipa_path: None,
            install_busy: Arc::new(Mutex::new(false)),
            install_result: Arc::new(Mutex::new(None)),
            limits: Arc::new(Mutex::new(Limits::default())),
            limits_poll_started: false,
            force_limits_refresh: Arc::new(Mutex::new(false)),
            revoke_busy: Arc::new(Mutex::new(false)),
            renew_all_busy: Arc::new(Mutex::new(false)),
            avatar_colors: HashMap::new(),
            guide_textures,
            guide_index: 0,
            guide_app_name: String::new(),
        }
    }

    /// Запускает фоновый поток, который раз в ~1.5 сек обновляет device_status.
    /// Вызывается единожды при первом показе экрана Connect.
    fn start_status_polling(&mut self) {
        if self.poll_started {
            return;
        }
        self.poll_started = true;
        let status = Arc::clone(&self.device_status);
        thread::spawn(move || loop {
            let s = check_device_status();
            *status.lock().unwrap() = s;
            if s == DeviceStatus::Paired {
                break;
            }
            thread::sleep(Duration::from_millis(1500));
        });
    }

    fn start_pair(&mut self) {
        {
            let mut b = self.pairing.lock().unwrap();
            if *b {
                return;
            }
            *b = true;
        }
        *self.pair_result.lock().unwrap() = None;
        let log = self.log.clone();
        let pairing = Arc::clone(&self.pairing);
        let result = Arc::clone(&self.pair_result);
        let status = Arc::clone(&self.device_status);
        thread::spawn(move || {
            let ok = connect_device(&log);
            if ok {
                *status.lock().unwrap() = DeviceStatus::Paired;
            }
            *result.lock().unwrap() = Some(ok);
            *pairing.lock().unwrap() = false;
        });
    }

    fn start_login(&mut self) {
        {
            let mut b = self.login_busy.lock().unwrap();
            if *b {
                return;
            }
            *b = true;
        }
        *self.login_result.lock().unwrap() = None;
        self.login_error = None;

        let creds = Creds { email: self.email.clone(), password: self.password.clone() };
        let log = self.log.clone();
        let busy = Arc::clone(&self.login_busy);
        let result = Arc::clone(&self.login_result);
        let code_rx = Arc::clone(&self.code_tx);
        let waiting_2fa = Arc::clone(&self.waiting_2fa);

        thread::spawn(move || {
            let ok = ensure_certificate(&creds, code_rx, waiting_2fa, &log);
            if ok {
                save_creds(&creds);
            }
            *result.lock().unwrap() = Some(ok);
            *busy.lock().unwrap() = false;
        });
    }

    fn submit_2fa(&mut self) {
        if self.code_input.trim().is_empty() {
            return;
        }
        *self.code_tx.lock().unwrap() = Some(self.code_input.trim().to_string());
        self.code_input.clear();
    }

    /// ДЕМО-РЕЖИМ: имитирует успешную установку, не трогая Apple ID вообще —
    /// не запускает sideloader, не открывает PTY, не тратит недельный лимит
    /// App ID / переустановок. Нужен, чтобы проверить экран-инструкцию
    /// (5 листаемых скринов) без реального логина. Работает даже если .ipa
    /// не выбран и учётка не привязана — подставляет заглушку вместо имени.
    fn start_demo_install(&mut self) {
        self.guide_app_name = self
            .ipa_path
            .as_deref()
            .map(app_display_name_from_ipa_path)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "DemoApp".to_string());
        self.guide_index = 0;
        self.screen = Screen::Guide;
    }

    fn start_install(&mut self) {
        let (Some(creds), Some(path)) = (self.creds.clone(), self.ipa_path.clone()) else {
            return;
        };
        {
            // Проверяем оба флага: install и "Продлить всё" шарят один и тот
            // же code_tx/waiting_2fa (один Apple ID, один логин-процесс за
            // раз) — если оба запустятся одновременно, ответы 2FA/коды будут
            // долетать до случайного из двух процессов, а не до того, что
            // его реально спросил.
            let mut b = self.install_busy.lock().unwrap();
            let renew_busy = *self.renew_all_busy.lock().unwrap();
            if *b || renew_busy {
                return;
            }
            *b = true;
        }
        *self.install_result.lock().unwrap() = None;

        let log = self.log.clone();
        let busy = Arc::clone(&self.install_busy);
        let result = Arc::clone(&self.install_result);
        let code_rx = Arc::clone(&self.code_tx);
        let waiting_2fa = Arc::clone(&self.waiting_2fa);
        let force_refresh = Arc::clone(&self.force_limits_refresh);
        let limits = Arc::clone(&self.limits);

        thread::spawn(move || {
            // Кладём копию .ipa в свою папку (~/averstor/ipas/) — дальше работаем
            // с ней, чтобы не зависеть от исходного файла, который пользователь
            // может переместить или удалить.
            let stored_path = match copy_ipa_to_store(&path) {
                Ok(p) => p,
                Err(e) => {
                    log.push(format!("Не удалось скопировать .ipa в свою папку: {e}"));
                    path.clone()
                }
            };

            // Снимок App ID ДО установки — по нему после install() поймём,
            // какие bundle_id реально появились новыми (включая расширения:
            // виджеты, NotificationServiceExtension и т.п. — каждый из них
            // отдельный App ID и тоже расходует недельный лимит в 10 штук).
            // Раньше "новизна" определялась парсингом строки "App IDs needed:"
            // из PTY-лога установки — она есть не в любом выводе sideloader,
            // из-за чего record_app_id_creation() мог вообще ни разу не
            // вызваться и счётчик "App IDs за 7 дней" замирал на 0/10.
            let bundle_ids_before: std::collections::HashSet<String> = {
                limits.lock().unwrap().apps.iter().map(|a| a.bundle_id.clone()).collect()
            };

            let lines_before = log.snapshot().len();
            let ok = install_ipa(&creds, &stored_path, code_rx.clone(), waiting_2fa.clone(), &log);
            if ok {
                // Сохраняем время успешной подписи — от него считаем 7 дней в UI.
                save_last_signed_now();
                let snapshot = log.snapshot();
                let start = lines_before.min(snapshot.len());

                // Достаём корневой bundle id (для карты "Продлить всё") —
                // best-effort, как раньше.
                let root_bundle = extract_bundle_from_lines(&snapshot[start..]);
                if let Some(bundle_id) = &root_bundle {
                    remember_ipa_for_bundle(bundle_id, &stored_path);
                } else {
                    log.push("Не удалось определить bundle id из вывода — \"Продлить всё\" может не подхватить это приложение автоматически.".into());
                }

                // Актуальный список App ID напрямую у Apple — источник истины
                // для подсчёта СКОЛЬКО именно новых App ID реально создалось
                // за этот install (root + все расширения разом).
                let lines_before2 = log.snapshot().len();
                let list_ok = run_sideloader_interactive(
                    &["app-id", "list", "-i"],
                    &creds.email,
                    &creds.password,
                    code_rx,
                    waiting_2fa,
                    &log,
                );
                if list_ok {
                    let snapshot2 = log.snapshot();
                    let start2 = lines_before2.min(snapshot2.len());
                    let output = snapshot2[start2..].join("\n");
                    let apps_after = parse_app_id_list(&output);

                    // Каждый App ID этого приложения (корень + все расширения)
                    // учитывается в недельном счётчике ТВЁРДО при каждой
                    // установке ИЛИ обновлении — не только при первом появлении
                    // bundle id. Счётчик всё равно не уменьшается при удалении
                    // приложения (см. record_app_id_creation/app_id_creations_last_7_days),
                    // так что это осознанное решение, а не двойной учёт по ошибке.
                    let group_bundle_ids: Vec<String> = match &root_bundle {
                        Some(root) => group_apps(&apps_after)
                            .into_iter()
                            .find(|g| &g.root_bundle_id == root)
                            .map(|g| g.all_bundle_ids)
                            .unwrap_or_else(|| vec![root.clone()]),
                        None => apps_after
                            .iter()
                            .filter(|a| !bundle_ids_before.contains(&a.bundle_id))
                            .map(|a| a.bundle_id.clone())
                            .collect(),
                    };
                    for _ in 0..group_bundle_ids.len().max(1) {
                        record_app_id_creation();
                    }

                    let mut l = limits.lock().unwrap();
                    l.apps = apps_after;
                    l.app_ids_used = l.apps.len();
                    l.has_data = true;
                }

                // Установка реально прошла — форсируем немедленный пересчёт
                // лимитов вместо ожидания обычного 5-минутного интервала.
                *force_refresh.lock().unwrap() = true;
            }
            *result.lock().unwrap() = Some(ok);
            *busy.lock().unwrap() = false;
        });
    }

    /// "Продлить всё" — переустанавливает (= переподписывает) подряд все
    /// .ipa из своей папки (~/averstor/ipas/). Каждая переустановка обновляет
    /// сертификат на свежие 7 дней для этого приложения.
    fn start_renew_all(&mut self) {
        // Чистим карту от записей с уже несуществующими файлами перед
        // началом — раньше такие "мёртвые" записи (оставшиеся, например, от
        // старого поведения copy_ipa_to_store, плодившего _1/_2 копии)
        // навсегда оседали в ipa_map.dat: каждый цикл "Продлить всё" видел
        // их, писал "Файл не найден, пропускаю" и ничего не делал — без
        // удаления это повторялось бы при каждом запуске бесконечно.
        let map = load_ipa_map();
        let (valid, dead): (Vec<_>, Vec<_>) = map.into_iter().partition(|(_, p)| std::path::Path::new(p).exists());
        if !dead.is_empty() {
            save_ipa_map(&valid);
            for (bundle, path) in &dead {
                self.log.push(format!("Убрал из списка продления (файл отсутствует): {bundle} -> {path}"));
            }
        }
        let map = valid;

        if map.is_empty() {
            return;
        }
        let Some(creds) = self.creds.clone() else { return };
        {
            // Симметрично start_install: не даём стартовать, если уже идёт
            // одиночная установка — оба процесса используют общий
            // code_tx/waiting_2fa для ввода 2FA-кода.
            let mut b = self.renew_all_busy.lock().unwrap();
            let install_busy = *self.install_busy.lock().unwrap();
            if *b || install_busy {
                return;
            }
            *b = true;
        }

        let busy = Arc::clone(&self.renew_all_busy);
        let code_rx = Arc::clone(&self.code_tx);
        let waiting_2fa = Arc::clone(&self.waiting_2fa);
        let log = self.log.clone();
        let force_refresh = Arc::clone(&self.force_limits_refresh);

        thread::spawn(move || {
            log.push(format!("Продлеваю все известные приложения ({} шт.)...", map.len()));

            // Перед продлением обязательно отзываем старый сертификат —
            // если этого не сделать, sideloader переиспользует его при install
            // и дата истечения не сдвинется на новые 7 дней.
            // Делаем один раз до цикла (не перед каждым приложением), потому
            // что сертификат один на весь Apple ID, а не на каждое приложение.
            if !ensure_certificate(&creds, Arc::clone(&code_rx), Arc::clone(&waiting_2fa), &log) {
                log.push("Не удалось подготовить сертификат, продление прервано.".into());
                *busy.lock().unwrap() = false;
                return;
            }

            let mut succeeded = 0usize;
            let mut failed: Vec<String> = Vec::new();

            for (bundle_id, path) in &map {
                if !std::path::Path::new(path).exists() {
                    log.push(format!("Файл не найден, пропускаю: {path}"));
                    continue;
                }
                if install_ipa(&creds, path, Arc::clone(&code_rx), Arc::clone(&waiting_2fa), &log) {
                    succeeded += 1;
                    // Обновляем время подписи сразу после каждого успешного
                    // приложения — иначе если последнее упадёт, у предыдущих
                    // всё равно будет правильное время отсчёта.
                    save_last_signed_now();
                    // Обновление (переустановка/продление) тоже засчитываем в
                    // недельный счётчик App ID — id-шки должны накапливаться
                    // и при установке, и при обновлении приложения, а
                    // обнуляться только через 7 дней, независимо от удаления.
                    record_app_id_creation();
                } else {
                    failed.push(bundle_id.clone());
                }
            }

            if failed.is_empty() {
                log.push(format!("Продление всех приложений завершено успешно ({succeeded} шт.)."));
            } else {
                log.push(format!(
                    "Продление завершено с ошибками: успешно {succeeded}, не удалось: {}. Подробности см. выше в журнале.",
                    failed.join(", ")
                ));
            }

            if succeeded > 0 {
                // Форсируем немедленный пересчёт лимитов вместо ожидания
                // обычного 5-минутного интервала опроса.
                *force_refresh.lock().unwrap() = true;
            }

            *busy.lock().unwrap() = false;
        });
    }

    fn logout(&mut self) {
        clear_creds();
        self.creds = None;
        self.email.clear();
        self.password.clear();
        // Сбрасываем флаг поллинга лимитов — при следующем входе нужен
        // свежий поток с новыми кредами (старый поток умрёт сам, когда
        // его вызовы sideloader начнут возвращать ошибку аутентификации).
        self.limits_poll_started = false;
        // После выхода возвращаемся на экран подключения, а не напрямую
        // на логин — устройство могло отключиться пока шла работа.
        self.screen = Screen::Connect;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(300));
        let l = self.lang;

        match self.screen {
            Screen::Connect => self.draw_connect(ctx, l),
            Screen::Login => self.draw_login(ctx, l),
            Screen::Main => self.draw_main(ctx, l),
            Screen::Guide => self.draw_guide(ctx, l),
        }
    }
}

impl App {
    fn draw_log_panel(&self, ctx: &egui::Context, l: &Lang) {
        // Плавающее окно поверх всего интерфейса, а не докпанель — раньше
        // TopBottomPanel сжимал центральную область, теперь журнал
        // разворачивается НАД остальным содержимым, не сдвигая его.
        // Стандартный title_bar у egui::Window слишком толстый — отключаем
        // его и рисуем свой тонкий заголовок, как было раньше.
        egui::Window::new("")
            .id(egui::Id::new("log_panel"))
            .title_bar(false)
            .resizable(true)
            .default_height(160.0)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
            .show(ctx, |ui| {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(l.log_title).monospace().size(11.0).color(egui::Color32::from_gray(140)));
                ui.separator();
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(300.0).show(ui, |ui| {
                    for line in self.log.snapshot() {
                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                    }
                });
            });
    }

    fn draw_connect(&mut self, ctx: &egui::Context, l: &Lang) {
        self.start_status_polling();

        let status = *self.device_status.lock().unwrap();

        // статус "сопряжено" — сразу уходим на следующий экран, эта панель
        // не должна быть видна вообще ни одного кадра.
        if status == DeviceStatus::Paired {
            self.screen = if self.creds.is_some() { Screen::Main } else { Screen::Login };
            return;
        }

        let pair_res = *self.pair_result.lock().unwrap();
        if pair_res.is_some() {
            *self.pair_result.lock().unwrap() = None;
        }

        // Пока комп видит iPhone по USB, но тот ещё не доверяет этому компу,
        // никакой кнопки нет — попытка сопряжения запускается сама и
        // повторяется каждые пару секунд, пока пользователь не нажмёт
        // "Доверять" на самом iPhone (после чего status станет Paired и мы
        // уйдём с этого экрана в самом начале функции).
        if status == DeviceStatus::ConnectedUsb {
            let busy = *self.pairing.lock().unwrap();
            if !busy {
                let should_attempt = match self.last_pair_attempt {
                    None => true,
                    Some(t) => t.elapsed() >= Duration::from_secs(2),
                };
                if should_attempt {
                    self.last_pair_attempt = Some(std::time::Instant::now());
                    self.start_pair();
                }
            }
        } else {
            self.last_pair_attempt = None;
        }

        self.draw_log_panel(ctx, l);

        let (status_text, status_color) = match status {
            DeviceStatus::NotConnected => (l.status_not_connected, egui::Color32::from_rgb(220, 70, 70)),
            DeviceStatus::ConnectedUsb => (l.status_connected_usb, egui::Color32::from_rgb(100, 180, 255)),
            DeviceStatus::Paired => unreachable!(),
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                if let Some(logo) = &self.logo {
                    let size = egui::vec2(150.0, 150.0);
                    ui.image((logo.id(), size));
                    ui.add_space(8.0);
                }
                ui.heading(egui::RichText::new(l.title).size(24.0));
                ui.add_space(20.0);
                ui.label(egui::RichText::new(l.connect_title).size(18.0));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(l.connect_hint)
                        .size(13.0)
                        .family(egui::FontFamily::Monospace)
                        .color(egui::Color32::from_gray(180)),
                );
                ui.add_space(18.0);

                ui.label(egui::RichText::new(status_text).size(14.0).monospace().color(status_color));
                ui.add_space(18.0);

                if status == DeviceStatus::ConnectedUsb {
                    ui.label(
                        egui::RichText::new(l.trust_hint)
                            .size(15.0)
                            .strong()
                            .color(egui::Color32::from_rgb(100, 180, 255)),
                    );
                    ui.add_space(10.0);
                    ui.spinner();
                }
            });
        });
    }

    fn draw_login(&mut self, ctx: &egui::Context, l: &Lang) {
        let res = *self.login_result.lock().unwrap();
        if let Some(ok) = res {
            *self.login_result.lock().unwrap() = None;
            if ok {
                self.creds = Some(Creds { email: self.email.clone(), password: self.password.clone() });
                self.screen = Screen::Main;
            } else {
                self.login_error = Some(l.login_wrong.into());
            }
        }

        self.draw_log_panel(ctx, l);

        let waiting_2fa = *self.waiting_2fa.lock().unwrap();
        let busy = *self.login_busy.lock().unwrap();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(50.0);
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new(l.login_title).size(20.0));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(l.login_hint)
                        .size(12.0)
                        .family(egui::FontFamily::Monospace)
                        .color(egui::Color32::from_gray(150)),
                );
                ui.add_space(18.0);

                if waiting_2fa {
                    ui.label(egui::RichText::new(l.login_2fa).size(14.0));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(l.login_2fa_hint)
                            .size(12.0)
                            .family(egui::FontFamily::Monospace)
                            .color(egui::Color32::from_gray(150)),
                    );
                    ui.add_space(8.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.code_input)
                            .hint_text("000000")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(160.0),
                    );
                    resp.request_focus();
                    ui.add_space(10.0);
                    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    let btn = colored_button(ui, l.btn_submit_code, egui::Color32::from_rgb(80, 210, 80), 140.0, 30.0);
                    if btn.clicked() || enter {
                        self.submit_2fa();
                    }
                } else {
                    ui.label(egui::RichText::new(l.login_email).size(13.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.email)
                            .hint_text("you@icloud.com")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(280.0),
                    );
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(l.login_password).size(13.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(280.0),
                    );

                    if let Some(ref err) = self.login_error {
                        ui.add_space(8.0);
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 60, 60),
                            egui::RichText::new(err.as_str()).size(13.0).family(egui::FontFamily::Monospace),
                        );
                    }

                    ui.add_space(16.0);
                    if busy {
                        ui.spinner();
                    } else {
                        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                        let btn = colored_button(ui, l.btn_login, egui::Color32::from_rgb(100, 180, 255), 160.0, 32.0);
                        if (btn.clicked() || enter) && !self.email.trim().is_empty() && !self.password.is_empty() {
                            self.start_login();
                        }
                    }
                }
            });
        });
    }

    fn draw_main(&mut self, ctx: &egui::Context, l: &Lang) {
        let res = *self.install_result.lock().unwrap();
        if let Some(ok) = res {
            *self.install_result.lock().unwrap() = None;
            if ok {
                // Успешная установка — показываем листаемую инструкцию,
                // как разрешить приложение от недоверенного разработчика.
                self.guide_app_name = self
                    .ipa_path
                    .as_deref()
                    .map(app_display_name_from_ipa_path)
                    .unwrap_or_default();
                self.guide_index = 0;
                self.screen = Screen::Guide;
                return;
            }
        }

        self.draw_log_panel(ctx, l);

        let waiting_2fa = *self.waiting_2fa.lock().unwrap();
        let install_busy = *self.install_busy.lock().unwrap();

        egui::TopBottomPanel::top("top_panel")
            .frame(egui::Frame::default()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(egui::Margin::symmetric(14.0, 0.0)))
            .show(ctx, |ui| {
            ui.add_space(8.0);

            // ── Строка 1: лого + название — строго по центру окна, не глядя
            // на кнопку выхода (раньше центр смещался из-за резерва под неё).
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    let logo_size = 22.0_f32;
                    let gap = 4.0_f32;
                    let title_galley = ui.fonts(|f| f.layout_no_wrap(l.title.into(), egui::FontId::proportional(18.0), egui::Color32::WHITE));
                    let title_w = title_galley.rect.width();
                    let content_w = if self.logo.is_some() { logo_size + gap + title_w } else { title_w };
                    // Центрируем содержимое горизонтальной полоски относительно
                    // её собственной ширины (которая = ширине окна с отступами).
                    let avail = ui.available_width();
                    let offset = (avail - content_w) / 2.0;
                    if offset > 0.0 {
                        ui.add_space(offset);
                    }
                    if let Some(logo) = &self.logo {
                        ui.image((logo.id(), egui::vec2(logo_size, logo_size)));
                        ui.add_space(gap);
                    }
                    ui.heading(egui::RichText::new(l.title).size(18.0));
                });
            });

            ui.add_space(6.0);

            // ── Строка 2: ссылка на Apple Developer слева, кнопка выхода справа.
            ui.horizontal(|ui| {
                let link = ui.add(
                    egui::Label::new(
                        egui::RichText::new(l.dev_portal_link)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(100, 180, 255))
                            .underline(),
                    )
                    .sense(egui::Sense::click()),
                );
                if link.clicked() {
                    open_url("https://developer.apple.com/account");
                }
                if link.hovered() {
                    ctx.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let logout_text = egui::RichText::new(l.btn_logout).size(12.0).color(egui::Color32::from_gray(170));
                    if ui.add(egui::Button::new(logout_text).frame(false)).clicked() {
                        self.logout();
                    }
                });
            });
            ui.add_space(8.0);
        });

        self.start_limits_poll();
        let limits = self.limits.lock().unwrap().clone();

        egui::CentralPanel::default().show(ctx, |ui| {
            // Делим на две колонки: левая — установка IPA, правая — лимиты
            let col_sep = egui::Color32::from_gray(30);
            let total = ui.available_width();
            let right_w = 220.0_f32.min(total * 0.42);
            let left_w = total - right_w - 1.0;

            ui.horizontal_top(|ui| {
                // ── ЛЕВАЯ КОЛОНКА ────────────────────────────────────────────
                ui.allocate_ui(egui::vec2(left_w, ui.available_height()), |ui| {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        if let Some(ref path) = self.ipa_path {
                            let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            ui.label(egui::RichText::new(name).size(14.0).monospace());
                        } else {
                            ui.label(
                                egui::RichText::new(l.no_ipa).size(13.0).family(egui::FontFamily::Monospace).color(egui::Color32::from_gray(150)),
                            );
                        }
                        ui.add_space(12.0);

                        let pick_btn = colored_button(ui, l.btn_pick_ipa, egui::Color32::from_rgb(100, 180, 255), 180.0, 30.0);
                        if pick_btn.clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("IPA", &["ipa"]).pick_file() {
                                self.ipa_path = Some(path.to_string_lossy().to_string());
                            }
                        }

                        ui.add_space(20.0);

                        if waiting_2fa {
                            ui.label(egui::RichText::new(l.login_2fa).size(14.0));
                            ui.add_space(6.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.code_input)
                                    .hint_text("000000")
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(160.0),
                            );
                            resp.request_focus();
                            ui.add_space(8.0);
                            let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                            let btn = colored_button(ui, l.btn_submit_code, egui::Color32::from_rgb(80, 210, 80), 140.0, 28.0);
                            if btn.clicked() || enter {
                                self.submit_2fa();
                            }
                        } else if install_busy {
                            ui.spinner();
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(l.status_working).size(13.0).family(egui::FontFamily::Monospace));
                        } else if self.ipa_path.is_some() {
                            // Кнопка "Подписать и установить" занимает место только
                            // когда выбран файл — пока ничего не выбрано, её вообще
                            // нет в дереве виджетов (0 места), а не просто disabled.
                            let btn = colored_button(ui, l.btn_install, egui::Color32::from_rgb(80, 210, 80), 220.0, 34.0);
                            if btn.clicked() {
                                self.start_install();
                            }
                            ui.add_space(16.0);
                        }

                        if !waiting_2fa && !install_busy {
                            // Демо-кнопка — открывает экран-инструкцию (5 скринов)
                            // без реального обращения к Apple ID. Полезно, когда
                            // недельный лимит на текущих аккаунтах уже исчерпан,
                            // а проверить/показать сам гайд всё равно нужно.
                            let demo_btn = colored_button(
                                ui,
                                l.btn_demo_install,
                                egui::Color32::from_rgb(90, 90, 100),
                                220.0,
                                28.0,
                            );
                            if demo_btn.clicked() {
                                self.start_demo_install();
                            }
                            ui.add_space(16.0);
                        }

                        // ── "Продлить всё" ──────────────────────────────────────
                        // Большая зелёная кнопка, продлевающая (= переподписывающая)
                        // подряд все .ipa из своей папки (~/averstor/ipas/), которые
                        // уже когда-либо были установлены через эту программу.
                        // Видна всегда, независимо от того, выбран ли новый файл.
                        let renew_busy = *self.renew_all_busy.lock().unwrap();
                        let history_len = count_existing_ipas_and_clean();
                        if !waiting_2fa && !install_busy {
                            ui.label(
                                egui::RichText::new(l.renew_all_title)
                                    .size(13.0)
                                    .color(egui::Color32::from_gray(170)),
                            );
                            ui.add_space(8.0);
                            let renew_enabled = !renew_busy && history_len > 0;
                            let btn_label = if renew_busy {
                                l.renew_all_busy_label.to_string()
                            } else {
                                format!("{} ({})", l.btn_renew_all, history_len)
                            };
                            let btn = big_green_button(ui, &btn_label, renew_enabled, 220.0, 34.0);
                            if btn.clicked() {
                                self.start_renew_all();
                            }
                        }
                    });
                });

                // Вертикальный разделитель
                let sep_rect = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(1.0, ui.available_height()),
                );
                ui.painter().rect_filled(sep_rect, 0.0, col_sep);
                ui.add_space(1.0);

                // ── ПРАВАЯ КОЛОНКА: лимиты ───────────────────────────────────
                ui.allocate_ui(egui::vec2(right_w, ui.available_height()), |ui| {
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        let gray = egui::Color32::from_gray(140);

                        ui.label(egui::RichText::new("Лимиты аккаунта").size(12.0).color(gray).monospace());
                        ui.add_space(8.0);

                        if limits.loading && !limits.has_data {
                            // Спиннер на весь блок только при самой первой
                            // загрузке, когда показывать совсем нечего.
                            // Фоновые обновления (раз в 5 минут) не должны
                            // прятать уже отрисованный список приложений —
                            // иначе он "мигал" бы каждые 5 минут, см. комментарий
                            // у поля has_data в Limits.
                            ui.spinner();
                            ui.label(egui::RichText::new("Загрузка...").size(11.0).color(gray).monospace());
                        } else if limits.error.is_some() && !limits.has_data {
                            // Ошибка показывается только если у нас вообще
                            // никогда не было успешных данных — если данные
                            // уже есть (просто временный сбой фонового
                            // обновления), ничего не закрываем, обычная
                            // отрисовка списка ниже покажет старые данные.
                            ui.label(
                                egui::RichText::new(limits.error.as_deref().unwrap_or_default())
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(200, 60, 60))
                                    .monospace(),
                            );
                        } else {
                            // ── App IDs (недельный лимит создания) ───────────
                            // Apple и sideloader-cli не отдают напрямую "сколько
                            // App ID создано за последние 7 дней" — есть только
                            // `app-id list`, который показывает текущие активные
                            // App ID, а удаление недельный счётчик создания у
                            // Apple не уменьшает. Поэтому считаем приближённо:
                            // сама программа запоминает момент каждого СВОЕГО
                            // создания нового App ID (app_id_creations.dat) и
                            // считает, сколько их попадает в последние 7 дней.
                            // Если App ID создавались не через эту программу,
                            // или это первый запуск после того, как лимит уже
                            // был частично израсходован раньше — число может
                            // не совпадать с тем, что реально видит Apple.
                            let creations_7d = app_id_creations_last_7_days();
                            let creations_color = if creations_7d >= 10 {
                                egui::Color32::from_rgb(220, 60, 60)
                            } else if creations_7d >= 7 {
                                egui::Color32::from_rgb(230, 160, 0)
                            } else {
                                egui::Color32::from_rgb(100, 180, 255)
                            };
                            draw_limit_bar(ui, creations_7d, 10,
                                creations_color,
                                &format!("App IDs за 7 дней: {creations_7d}/10"))
                                .on_hover_text(
                                    "Сколько НОВЫХ App ID было создано через эту программу \
                                     за последние 7 дней (скользящее окно, а не с понедельника).\n\
                                     Apple даёт бесплатному аккаунту лимит на СОЗДАНИЕ новых \
                                     App ID — как правило, 10 в неделю. Удаление App ID (🗑) этот \
                                     счётчик не уменьшает, так же как и переустановка/продление \
                                     уже существующего приложения — лимит расходуется только при \
                                     регистрации НОВОГО bundle id.\n\
                                     Это приближённый счётчик: он учитывает только то, что было \
                                     создано через эту программу, и может не совпадать с реальным \
                                     значением у Apple, если App ID создавались другим способом.",
                                );


                            ui.add_space(10.0);

                            // ── Приложения ───────────────────────────────────
                            // ВАЖНО: используем group_apps(), а не "сырой" список —
                            // sideloader выдаёт отдельный App ID на каждое расширение
                            // (Widget, IntentsExtension, NotificationServiceExtension и
                            // т.п.), поэтому одно установленное приложение раньше
                            // показывалось как 5-7 "одинаковых" строк подряд.
                            let groups = group_apps(&limits.apps);
                            let busy = *self.revoke_busy.lock().unwrap();
                            let mut to_revoke: Option<Vec<String>> = None;

                            draw_limit_bar(ui, groups.len(), 3,
                                egui::Color32::from_rgb(80, 210, 80),
                                &format!("Приложения: {}/3", groups.len()))
                                .on_hover_text(
                                    "Сколько РАЗНЫХ приложений подписано прямо сейчас.\n\
                                     Бесплатный Apple ID разрешает не больше 3 одновременно \
                                     установленных приложений — все расширения одного приложения \
                                     (виджеты, notification service и т.п.) считаются как одно.\n\
                                     SideStore — это сам инструмент подписи, он тоже занимает один \
                                     из 3 слотов, если ты его себе ставил.",
                                );

                            ui.add_space(8.0);

                            if groups.is_empty() {
                                ui.label(egui::RichText::new("нет установленных приложений").size(10.0).color(gray).monospace());
                            } else {
                                // Считаем время до истечения из last_signed.dat —
                                // Apple не обновляет дату App ID при переподписи,
                                // поэтому надёжнее хранить время последней подписи локально
                                // и показывать обратный отсчёт 7 дней от него.
                                let signed_seconds_left: Option<i64> = load_last_signed().map(|t| {
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs() as i64;
                                    t + 7 * 86400 - now
                                });

                                for app in &groups {
                                    ui.add_space(6.0);
                                    // Приоритет: локальное время подписи (точное) > дата Apple (может быть устаревшей)
                                    let effective_secs = signed_seconds_left.or(app.seconds_left);
                                    let (days_color, days_text) = match effective_secs {
                                        Some(s) if s <= 24 * 3600       => (egui::Color32::from_rgb(220, 60, 60),  format_days_hours(s)),
                                        Some(s) if s <= 3 * 24 * 3600   => (egui::Color32::from_rgb(230, 160, 0),  format_days_hours(s)),
                                        Some(s)                          => (egui::Color32::from_rgb(80, 210, 80),  format_days_hours(s)),
                                        None                             => (gray,                                  "?".into()),
                                    };

                                    // Название + аватарка-логотип приложения + кнопка удаления.
                                    let color_idx = self.avatar_colors
                                        .get(&app.root_bundle_id)
                                        .copied()
                                        .unwrap_or_else(|| default_avatar_index(&app.root_bundle_id));
                                    ui.horizontal(|ui| {
                                        let clicked = draw_app_header_clickable(ui, &app.name, &app.root_bundle_id, color_idx);
                                        if clicked {
                                            let next = (color_idx + 1) % AVATAR_PALETTE.len();
                                            self.avatar_colors.insert(app.root_bundle_id.clone(), next);
                                        }
                                        ui.add_space(4.0);
                                        let extra = if app.extra_count > 0 {
                                            format!(" (+{} расш.)", app.extra_count)
                                        } else {
                                            String::new()
                                        };
                                        let name_resp = ui.label(
                                            egui::RichText::new(format!("{}{}", app.name, extra))
                                                .size(11.0)
                                                .strong(),
                                        );
                                        if app.extra_count > 0 {
                                            name_resp.on_hover_text(format!(
                                                "У этого приложения {} доп. App ID, кроме основного.\n\
                                                 Это не отдельные приложения, а его части — виджеты, \
                                                 расширения уведомлений (NotificationServiceExtension), \
                                                 интенты Siri (IntentsExtension) и т.п.\n\
                                                 У iOS-приложений с расширениями каждая часть регистрируется \
                                                 как свой App ID, но занимают они вместе только ОДИН слот \
                                                 в лимите «Приложения: x/3».",
                                                app.extra_count,
                                            ));
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let btn = ui.add_enabled(
                                                !busy,
                                                egui::Button::new(egui::RichText::new("🗑").size(11.0)).small(),
                                            ).on_hover_text(
                                                "Удалить это приложение и все его App ID \
                                                 (освобождает слот в лимите «Приложения»).",
                                            );
                                            if btn.clicked() {
                                                to_revoke = Some(app.all_bundle_ids.clone());
                                            }
                                        });
                                    });

                                    // Прогресс-бар для этого приложения: 7 дней — полный,
                                    // дальше плавно опускается по мере истечения сертификата.
                                    // Доля считается напрямую из секунд (а не из округлённых
                                    // дней), поэтому бар плавно идёт вниз внутри последних
                                    // суток, а не "застывает" на одном делении на весь день.
                                    let fraction = effective_secs
                                        .map(|s| (s.max(0) as f32) / (7.0 * 86400.0))
                                        .unwrap_or(0.0)
                                        .clamp(0.0, 1.0);
                                    draw_limit_bar_fraction(ui, fraction, days_color,
                                        &format!("{} до переподписи", days_text))
                                        .on_hover_text(
                                            "Через сколько времени истекает сертификат подписи \
                                             этого приложения — после этого iOS перестанет его \
                                             запускать, пока не переустановишь/переподпишешь.\n\
                                             Бесплатный сертификат живёт 7 дней, поэтому шкала \
                                             полная при 7 днях и опускается до 0.",
                                        );
                                    ui.label(
                                        egui::RichText::new(&app.root_bundle_id)
                                            .size(9.0)
                                            .color(egui::Color32::from_gray(100))
                                            .monospace(),
                                    );
                                }
                            }

                            if let Some(ids) = to_revoke {
                                self.revoke_app_group(ids);
                            }
                        }
                    });
                });
            });
        });
    }

    /// Экран-инструкция из 5 листаемых скринов, показывается сразу после
    /// успешной установки: наверху счётчик "N/5", сама картинка шага,
    /// под ней текст поздравления с названием приложения, снизу — кнопки
    /// "<" / ">" для листания и "Готово" для возврата на главный экран.
    fn draw_guide(&mut self, ctx: &egui::Context, l: &Lang) {
        let total = self.guide_textures.len().max(1);
        if self.guide_index >= total {
            self.guide_index = total - 1;
        }

        egui::TopBottomPanel::top("guide_top_panel")
            .frame(egui::Frame::default()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}/{}", self.guide_index + 1, total))
                            .size(15.0)
                            .monospace()
                            .color(egui::Color32::from_gray(190)),
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);

            // ── Картинка текущего шага, вписанная в доступную ширину ───────
            ui.vertical_centered(|ui| {
                if let Some(Some(tex)) = self.guide_textures.get(self.guide_index) {
                    let avail_w = (ui.available_width() - 24.0).max(50.0);
                    let avail_h = (ui.available_height() - 120.0).max(50.0);
                    let size = tex.size_vec2();
                    let scale = (avail_w / size.x).min(avail_h / size.y).min(1.0);
                    let draw_size = size * scale;
                    ui.image((tex.id(), draw_size));
                } else {
                    ui.add_space(40.0);
                    ui.label(
                        egui::RichText::new("(изображение недоступно)")
                            .size(13.0)
                            .color(egui::Color32::from_gray(120)),
                    );
                    ui.add_space(40.0);
                }
            });

            ui.add_space(16.0);

            // ── Текст поздравления с подстановкой имени приложения ──────────
            let app_name = if self.guide_app_name.trim().is_empty() {
                "приложения".to_string()
            } else {
                self.guide_app_name.clone()
            };
            let congrats = l.guide_congrats_tmpl.replace("{name}", &app_name);
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(congrats).size(14.0)));
            });

            ui.add_space(20.0);

            // ── Навигация "<" / ">" + "Готово" ──────────────────────────────
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 260.0).max(0.0) / 2.0);

                    let can_prev = self.guide_index > 0;
                    let can_next = self.guide_index + 1 < total;

                    let prev = ui.add_enabled(
                        can_prev,
                        egui::Button::new(egui::RichText::new("<").size(18.0).strong())
                            .min_size(egui::vec2(44.0, 36.0)),
                    );
                    if prev.clicked() {
                        self.guide_index = self.guide_index.saturating_sub(1);
                    }

                    ui.add_space(12.0);

                    let done = big_green_button(ui, l.btn_guide_done, true, 140.0, 36.0);
                    if done.clicked() {
                        self.screen = Screen::Main;
                    }

                    ui.add_space(12.0);

                    let next = ui.add_enabled(
                        can_next,
                        egui::Button::new(egui::RichText::new(">").size(18.0).strong())
                            .min_size(egui::vec2(44.0, 36.0)),
                    );
                    if next.clicked() {
                        self.guide_index = (self.guide_index + 1).min(total - 1);
                    }
                });
            });

            ui.add_space(10.0);
        });
    }
}

/// Рисует прогресс-бар лимита с подписью снизу (целочисленные used/max).
fn draw_limit_bar(ui: &mut egui::Ui, used: usize, max: usize, color: egui::Color32, label: &str) -> egui::Response {
    let fraction = if max == 0 { 0.0 } else { (used as f32 / max as f32).clamp(0.0, 1.0) };
    draw_limit_bar_fraction(ui, fraction, color, label)
}

/// То же самое, но принимает готовую долю заполнения (0.0–1.0) напрямую —
/// нужно для сертификата, где остаток нужно показывать с точностью до часов,
/// а не только целыми днями (иначе прогресс-бар "застывал" на весь последний
/// день, хотя реально оставалось от 23 часов до нескольких минут).
fn draw_limit_bar_fraction(ui: &mut egui::Ui, fraction: f32, color: egui::Color32, label: &str) -> egui::Response {
    let fraction = fraction.clamp(0.0, 1.0);
    let bar_h = 10.0_f32;
    let gap = 3.0_f32;
    let text_h = 12.0_f32;
    let bar_w = ui.available_width() - 8.0;
    let total_h = bar_h + gap + text_h;

    // Выделяем ОДИН прямоугольник на весь блок (бар + подпись под ним) и
    // интерактим его сразу — раньше hover/tooltip навешивался на response
    // от ui.vertical(), которая не всегда покрывала всю площадь надёжно,
    // из-за чего подсказка всплывала только у соседних виджетов (названий),
    // а не у самого бара с подписью.
    let (rect, response) = ui.allocate_exact_size(egui::vec2(bar_w, total_h), egui::Sense::hover());
    let bar_rect = egui::Rect::from_min_size(rect.min, egui::vec2(bar_w, bar_h));
    let painter = ui.painter();

    // Фон
    painter.rect_filled(bar_rect, 3.0, egui::Color32::from_gray(30));
    // Заполненная часть
    if fraction > 0.0 {
        let filled = egui::Rect::from_min_size(bar_rect.min, egui::vec2(bar_rect.width() * fraction, bar_rect.height()));
        painter.rect_filled(filled, 3.0, color);
    }
    // Рамка
    painter.rect_stroke(bar_rect, 3.0, egui::Stroke::new(1.0, egui::Color32::from_gray(50)));

    // Подпись под баром
    painter.text(
        egui::pos2(rect.min.x, bar_rect.max.y + gap),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(160),
    );

    response
}

fn main() -> eframe::Result<()> {
    let title = detect_lang().title;
    eframe::run_native(
        title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([640.0, 560.0])
                .with_min_inner_size([480.0, 420.0]),
            ..Default::default()
        },
        Box::new(|cc| Box::new(App::new(cc))),
    )
}
