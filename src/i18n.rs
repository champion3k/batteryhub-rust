//! UI language: English / Russian. The choice is stored in the registry and used by the taskbar
//! strip menu, tooltips and messages. The first start follows the Windows display language.
use std::sync::atomic::{AtomicU8, Ordering};

use windows::Win32::Globalization::GetUserDefaultUILanguage;

const SETTINGS_KEY: &str = "Software\\BatteryHub\\Taskbar";
const LANGUAGE_VALUE: &str = "Language";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ru => "Русский",
        }
    }

    fn from_code(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "en" | "english" => Some(Self::En),
            "ru" | "russian" => Some(Self::Ru),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0xFF);

fn system_lang() -> Lang {
    // LANGID low 10 bits = primary language. 0x19 = Russian.
    let id = unsafe { GetUserDefaultUILanguage() };
    if id & 0x03FF == 0x0019 { Lang::Ru } else { Lang::En }
}

/// Reads the saved choice (or the Windows language on the first run).
pub fn init() {
    let lang = crate::util::reg_get_string(SETTINGS_KEY, LANGUAGE_VALUE)
        .as_deref()
        .and_then(Lang::from_code)
        .unwrap_or_else(system_lang);
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        _ => Lang::En,
    }
}

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
    crate::util::reg_set_string(SETTINGS_KEY, LANGUAGE_VALUE, lang.code());
}

/// Fixed UI strings.
pub struct Texts {
    pub language: &'static str,
    pub refresh_devices: &'static str,
    pub refresh_levels: &'static str,
    pub refresh_every: &'static str,
    pub refresh_on_change: &'static str,
    pub background: &'static str,
    pub charging_pulse: &'static str,
    pub position: &'static str,
    pub next_to_tray: &'static str,
    pub height: &'static str,
    pub centred: &'static str,
    pub show_widget_on: &'static str,
    pub all_monitors: &'static str,
    pub main_monitor_only: &'static str,
    pub chosen_monitor_gone: &'static str,
    pub autostart: &'static str,
    pub auto_update: &'static str,
    pub silent_update: &'static str,
    pub request_support: &'static str,
    pub check_updates: &'static str,
    pub checking_updates: &'static str,
    pub exit: &'static str,
    pub history: &'static str,
    pub icon: &'static str,
    pub icon_file: &'static str,
    pub icon_auto: &'static str,
    pub icon_reset: &'static str,
    pub icon_size: &'static str,
    pub icon_not_found: &'static str,
    pub dnd: &'static str,
    pub red_warning: &'static str,
    pub default_suffix: &'static str,
    pub on_battery: &'static str,
    pub charging: &'static str,
    pub fully_charged: &'static str,
    pub disconnected: &'static str,
    pub asleep_last: &'static str,
    pub asleep_suffix: &'static str,
    pub full_suffix: &'static str,
    pub charging_suffix: &'static str,
    pub no_devices: &'static str,
}

pub static EN: Texts = Texts {
    language: "Language",
    refresh_devices: "Refresh devices",
    refresh_levels: "Refresh battery levels",
    refresh_every: "Refresh battery every",
    refresh_on_change: "Refresh when a device connects or disconnects",
    background: "Background behind taskbar text (for translucent taskbars)",
    charging_pulse: "Pulse the charging bolt",
    position: "Position on the taskbar",
    next_to_tray: "Next to the notification area",
    height: "Height on the taskbar",
    centred: "Centred",
    show_widget_on: "Show widget on",
    all_monitors: "All monitors",
    main_monitor_only: "Main monitor only",
    chosen_monitor_gone: "Chosen monitor (not connected, shown on main)",
    autostart: "Automatically start with Windows",
    auto_update: "Automatically check for updates",
    silent_update: "Install updates silently",
    request_support: "Request support for a device...",
    check_updates: "Check for updates...",
    checking_updates: "Checking for updates...",
    exit: "Exit",
    history: "Battery history...",
    icon: "Icon",
    icon_file: "Choose file...",
    icon_auto: "Find by device name",
    icon_reset: "Reset icon",
    icon_size: "Icon size",
    icon_not_found: "No device image found. Choose a PNG/ICO file instead.",
    dnd: "Do not disturb",
    red_warning: "Red warning at or below:",
    default_suffix: "  (default)",
    on_battery: "on battery",
    charging: "charging",
    fully_charged: "fully charged",
    disconnected: "disconnected",
    asleep_last: "asleep, last known level",
    asleep_suffix: " (asleep)",
    full_suffix: " (full)",
    charging_suffix: " (charging)",
    no_devices: "BatteryHub: no devices found",
};

pub static RU: Texts = Texts {
    language: "Язык",
    refresh_devices: "Обновить устройства",
    refresh_levels: "Обновить заряд",
    refresh_every: "Обновлять заряд каждые",
    refresh_on_change: "Обновлять при подключении/отключении устройства",
    background: "Фон под текстом панели задач (для прозрачных панелей)",
    charging_pulse: "Пульсация молнии при зарядке",
    position: "Положение на панели задач",
    next_to_tray: "Рядом с областью уведомлений",
    height: "Высота на панели задач",
    centred: "По центру",
    show_widget_on: "Показывать виджет на",
    all_monitors: "Все мониторы",
    main_monitor_only: "Только основной монитор",
    chosen_monitor_gone: "Выбранный монитор (не подключён, показывается на основном)",
    autostart: "Запускать при входе в Windows",
    auto_update: "Автоматически проверять обновления",
    silent_update: "Устанавливать обновления без запросов",
    request_support: "Запросить поддержку устройства...",
    check_updates: "Проверить обновления...",
    checking_updates: "Проверка обновлений...",
    exit: "Выход",
    history: "История заряда...",
    icon: "Иконка",
    icon_file: "Выбрать файл...",
    icon_auto: "Найти по названию",
    icon_reset: "Сбросить иконку",
    icon_size: "Размер иконки",
    icon_not_found: "Изображение устройства не найдено. Выберите PNG/ICO вручную.",
    dnd: "Не беспокоить",
    red_warning: "Красный при заряде не выше:",
    default_suffix: "  (по умолчанию)",
    on_battery: "от батареи",
    charging: "заряжается",
    fully_charged: "полностью заряжено",
    disconnected: "отключено",
    asleep_last: "спит, последний известный заряд",
    asleep_suffix: " (спит)",
    full_suffix: " (полный)",
    charging_suffix: " (заряжается)",
    no_devices: "BatteryHub: устройства не найдены",
};

pub fn t() -> &'static Texts {
    match current() {
        Lang::Ru => &RU,
        Lang::En => &EN,
    }
}

/// Strings of the "Request support for a device" window.
pub struct ReportTexts {
    pub title: &'static str,
    pub intro: &'static str,
    pub device: &'static str,
    pub scanning_devices: &'static str,
    pub model: &'static str,
    pub type_label: &'static str,
    pub type_names: [&'static str; 5],
    pub name: &'static str,
    pub notes: &'static str,
    pub preview: &'static str,
    pub scanning_status: &'static str,
    pub copy: &'static str,
    pub send: &'static str,
    pub close: &'static str,
    pub not_in_list: &'static str,
    pub pick_status: &'static str,
    pub title_prefix: &'static str,
    pub empty_model: &'static str,
    pub opened: &'static str,
    pub failed_copied: &'static str,
    pub failed: &'static str,
    pub copied_full: &'static str,
    pub copy_failed: &'static str,
}

pub static REPORT_EN: ReportTexts = ReportTexts {
    title: "BatteryHub: request support for a device",
    intro: "Is one of your devices not shown? Pick it below. Send opens a new issue on the project's GitHub page, filled in with what is shown here; you check it and submit it there (a GitHub account is needed).",
    device: "Device",
    scanning_devices: "Scanning devices...",
    model: "Device name (as it's sold)",
    type_label: "Type",
    type_names: ["mouse", "keyboard", "headset", "controller", "other"],
    name: "Your name (optional, shown in the GitHub issue)",
    notes: "Anything else we should know? (optional)",
    preview: "This is exactly what the GitHub issue will contain (in English):",
    scanning_status: "Scanning your devices...",
    copy: "Copy full report",
    send: "Send",
    close: "Close",
    not_in_list: "My device is not in this list",
    pick_status: "Pick the device that isn't shown. Devices with a vendor-specific interface or audio are listed first.",
    title_prefix: "Title:",
    empty_model: "(type the device name)",
    opened: "GitHub opened in your browser with the request filled in. Check it and click Create to submit it. Thank you!",
    failed_copied: "Couldn't open the browser. The request is copied: paste it into a new issue at github.com/champion3k/batteryhub-rust/issues.",
    failed: "Couldn't open the browser. Please open a new issue at github.com/champion3k/batteryhub-rust/issues.",
    copied_full: "The full report (with every device BatteryHub sees) is copied. Paste it into the GitHub issue if we ask for more details.",
    copy_failed: "Couldn't copy to the clipboard. Please try again.",
};

pub static REPORT_RU: ReportTexts = ReportTexts {
    title: "BatteryHub: запрос поддержки устройства",
    intro: "Устройства нет в списке? Выберите его ниже. «Отправить» откроет новую задачу на GitHub проекта с заполненными данными; проверьте её и отправьте там (нужен аккаунт GitHub).",
    device: "Устройство",
    scanning_devices: "Поиск устройств...",
    model: "Название устройства (как в продаже)",
    type_label: "Тип",
    type_names: ["мышь", "клавиатура", "наушники", "геймпад", "другое"],
    name: "Ваше имя (необязательно, будет видно в задаче GitHub)",
    notes: "Что ещё стоит знать? (необязательно)",
    preview: "Именно это попадёт в задачу на GitHub (текст на английском):",
    scanning_status: "Сканирую устройства...",
    copy: "Копировать полный отчёт",
    send: "Отправить",
    close: "Закрыть",
    not_in_list: "Моего устройства нет в списке",
    pick_status: "Выберите устройство, которого нет в списке. Устройства с vendor-интерфейсом или аудио идут первыми.",
    title_prefix: "Заголовок:",
    empty_model: "(введите название устройства)",
    opened: "GitHub открыт в браузере с заполненным запросом. Проверьте его и нажмите Create, чтобы отправить. Спасибо!",
    failed_copied: "Не удалось открыть браузер. Запрос скопирован: вставьте его в новую задачу на github.com/champion3k/batteryhub-rust/issues.",
    failed: "Не удалось открыть браузер. Откройте новую задачу на github.com/champion3k/batteryhub-rust/issues.",
    copied_full: "Полный отчёт (со всеми устройствами, которые видит BatteryHub) скопирован. Вставьте его в задачу GitHub, если попросят подробности.",
    copy_failed: "Не удалось скопировать в буфер обмена. Попробуйте ещё раз.",
};

pub fn report() -> &'static ReportTexts {
    match current() {
        Lang::Ru => &REPORT_RU,
        Lang::En => &REPORT_EN,
    }
}

pub struct HistoryTexts {
    pub title: &'static str,
    pub device: &'static str,
    pub range: &'static str,
    pub h24: &'static str,
    pub d7: &'static str,
    pub d30: &'static str,
    pub latest: &'static str,
    pub minimum: &'static str,
    pub maximum: &'static str,
    pub change: &'static str,
    pub samples: &'static str,
    pub rate: &'static str,
    pub eta: &'static str,
    pub no_data: &'static str,
    pub charging: &'static str,
    pub on_battery: &'static str,
    pub chart_title: &'static str,
    pub chart_sub: &'static str,
    pub insufficient: &'static str,
    pub generated: &'static str,
    pub theme: &'static str,
    pub light: &'static str,
    pub dark: &'static str,
    pub heatmap_title: &'static str,
    pub heatmap_sub: &'static str,
    pub event_charge: &'static str,
    pub event_unplug: &'static str,
    pub collapse: &'static str,
    pub expand: &'static str,
}

pub static HISTORY_EN: HistoryTexts = HistoryTexts {
    title: "Battery history",
    device: "Device",
    range: "Range",
    h24: "24 hours",
    d7: "7 days",
    d30: "30 days",
    latest: "Latest",
    minimum: "Minimum",
    maximum: "Maximum",
    change: "Change",
    samples: "samples",
    rate: "Discharge",
    eta: "Time to empty",
    no_data: "No history yet. Let BatteryHub run for a while.",
    charging: "Charging",
    on_battery: "On battery",
    chart_title: "Battery level",
    chart_sub: "Hover the chart for exact values",
    insufficient: "Not enough data for an estimate yet",
    generated: "Generated",
    theme: "Theme",
    light: "Light theme",
    dark: "Dark theme",
    heatmap_title: "Month at a glance",
    heatmap_sub: "Average charge by hour, last 30 days",
    event_charge: "on power",
    event_unplug: "unplugged",
    collapse: "Collapse",
    expand: "Expand",
};

pub static HISTORY_RU: HistoryTexts = HistoryTexts {
    title: "История заряда",
    device: "Устройство",
    range: "Период",
    h24: "24 часа",
    d7: "7 дней",
    d30: "30 дней",
    latest: "Сейчас",
    minimum: "Минимум",
    maximum: "Максимум",
    change: "Изменение",
    samples: "точек",
    rate: "Разряд",
    eta: "До нуля",
    no_data: "Истории пока нет. Дайте BatteryHub поработать.",
    charging: "Заряжается",
    on_battery: "От батареи",
    chart_title: "Уровень заряда",
    chart_sub: "Наведите на график для точных значений",
    insufficient: "Пока мало данных для прогноза",
    generated: "Сформировано",
    theme: "Тема",
    light: "Светлая тема",
    dark: "Тёмная тема",
    heatmap_title: "Месяц как на ладони",
    heatmap_sub: "Средний заряд по часам за 30 дней",
    event_charge: "на зарядке",
    event_unplug: "снято",
    collapse: "Свернуть",
    expand: "Развернуть",
};

pub fn history() -> &'static HistoryTexts {
    match current() {
        Lang::Ru => &HISTORY_RU,
        Lang::En => &HISTORY_EN,
    }
}

pub fn low_battery(name: &str, percent: i32) -> String {
    match current() {
        Lang::Ru => format!("Низкий заряд: {name} — {percent}%"),
        Lang::En => format!("Low battery: {name} — {percent}%"),
    }
}

pub fn charging_started(name: &str) -> String {
    match current() {
        Lang::Ru => format!("Зарядка началась: {name}"),
        Lang::En => format!("Charging started: {name}"),
    }
}

pub fn duration(hours: f64) -> String {
    match current() {
        Lang::Ru if hours < 1.0 => format!("{:.0} мин", (hours * 60.0).max(1.0)),
        Lang::Ru if hours < 24.0 => format!("{:.0} ч", hours.max(1.0)),
        Lang::Ru => format!("{:.0} д", (hours / 24.0).max(1.0)),
        Lang::En if hours < 1.0 => format!("{:.0} min", (hours * 60.0).max(1.0)),
        Lang::En if hours < 24.0 => format!("{:.0} h", hours.max(1.0)),
        Lang::En => format!("{:.0} d", (hours / 24.0).max(1.0)),
    }
}

pub fn fully_charged(name: &str) -> String {
    match current() {
        Lang::Ru => format!("{name}: полностью заряжено"),
        Lang::En => format!("{name}: fully charged"),
    }
}

pub fn update_title() -> &'static str {
    match current() {
        Lang::Ru => "BatteryHub — проверка обновлений",
        Lang::En => "BatteryHub - Check for updates",
    }
}

pub fn interval(minutes: u32) -> String {
    match (current(), minutes) {
        (Lang::Ru, 1) => "1 минута".into(),
        (Lang::Ru, 5) => "5 минут".into(),
        (Lang::Ru, 10) => "10 минут".into(),
        (Lang::Ru, 30) => "30 минут".into(),
        (Lang::Ru, 60) => "1 час".into(),
        (Lang::Ru, m) => format!("{m} минут"),
        (Lang::En, 1) => "1 minute".into(),
        (Lang::En, 5) => "5 minutes".into(),
        (Lang::En, 10) => "10 minutes".into(),
        (Lang::En, 30) => "30 minutes".into(),
        (Lang::En, 60) => "1 hour".into(),
        (Lang::En, m) => format!("{m} minutes"),
    }
}

pub fn px_left(px: u32) -> String {
    match current() {
        Lang::Ru => format!("{px} px влево"),
        Lang::En => format!("{px} px to the left"),
    }
}

pub fn px_up(px: i32) -> String {
    match current() {
        Lang::Ru => format!("{px} px вверх"),
        Lang::En => format!("{px} px up"),
    }
}

pub fn px_down(px: i32) -> String {
    match current() {
        Lang::Ru => format!("{px} px вниз"),
        Lang::En => format!("{px} px down"),
    }
}

pub fn custom_offset(px: i32, key: &str) -> String {
    match current() {
        Lang::Ru => format!("Вручную ({px} px, {key} в реестре)"),
        Lang::En => format!("Custom ({px} px, {key} in the registry)"),
    }
}

pub fn only(label: &str) -> String {
    match current() {
        Lang::Ru => format!("Только {label}"),
        Lang::En => format!("{label} only"),
    }
}

pub fn display(n: &str) -> String {
    match current() {
        Lang::Ru => format!("Экран {n}"),
        Lang::En => format!("Display {n}"),
    }
}

pub fn other_monitor() -> &'static str {
    match current() {
        Lang::Ru => "Другой монитор",
        Lang::En => "Other monitor",
    }
}

pub fn side(side: &str) -> String {
    match (current(), side) {
        (Lang::Ru, "left") => "слева".into(),
        (Lang::Ru, "right") => "справа".into(),
        (Lang::Ru, "above") => "сверху".into(),
        (Lang::Ru, "below") => "снизу".into(),
        (_, other) => other.into(),
    }
}

pub fn up_to_date(current_version: &str, latest: &str) -> String {
    match current() {
        Lang::Ru => format!("Обновлений нет.\n\nУ вас последняя версия ({current_version}).\nПоследний релиз на GitHub: {latest}."),
        Lang::En => format!("No new updates.\n\nYou have the latest version ({current_version}).\nLatest release on GitHub: {latest}."),
    }
}

pub fn check_failed(error: &str, repo: &str) -> String {
    match current() {
        Lang::Ru => format!("Не удалось проверить обновления:\n{error}\n\nНовые версии также на GitHub:\nhttps://github.com/{repo}/releases"),
        Lang::En => format!("Could not check for updates:\n{error}\n\nNew versions are also on GitHub:\nhttps://github.com/{repo}/releases"),
    }
}

pub fn update_available(from: &str, to: &str) -> String {
    match current() {
        Lang::Ru => format!(
            "Доступно обновление:\n\n        {from}  ->  {to}\n\nОбновить сейчас?\n\nУстановщик скачивается с GitHub, проверяется по SHA-256 и запускается; настройки и автозапуск сохраняются."
        ),
        Lang::En => format!(
            "A new update is available:\n\n        {from}  ->  {to}\n\nDo you want to update now?\n\nThe setup is downloaded from GitHub, checked against the release's SHA-256 checksum and started; it keeps your settings and autostart."
        ),
    }
}

pub fn could_not_start_setup(error: &str, path: &str) -> String {
    match current() {
        Lang::Ru => format!("Не удалось запустить установщик:\n{error}\n\n{path}"),
        Lang::En => format!("Could not start the setup:\n{error}\n\n{path}"),
    }
}

pub fn could_not_download(error: &str) -> String {
    match current() {
        Lang::Ru => format!("Не удалось скачать обновление:\n{error}"),
        Lang::En => format!("The update could not be downloaded:\n{error}"),
    }
}

pub fn update_finished(from: &str, to: &str) -> String {
    match current() {
        Lang::Ru => format!("Обновление установлено.\n\nBatteryHub обновлён:  {from}  ->  {to}\n\nНастройки и автозапуск сохранены."),
        Lang::En => format!("Update finished.\n\nBatteryHub was updated:  {from}  ->  {to}\n\nYour settings and autostart were kept."),
    }
}

pub fn auto_update_on() -> &'static str {
    match current() {
        Lang::Ru => "Автопроверка обновлений включена.\n\nBatteryHub ищет новую версию только при запуске (при входе в Windows или когда вы запускаете его вручную), а не во время работы. Если версия найдена, он спрашивает, обновляться ли (или, при включённой опции \"Устанавливать обновления без запросов\", устанавливает её в фоне).",
        Lang::En => "Automatic update check is on.\n\nBatteryHub looks for a new version only when it starts (at sign-in or when you start it), not while it runs. When there is one, it asks you whether to update (or, with \"Install updates silently\", installs it in the background).",
    }
}

pub fn silent_update_on() -> &'static str {
    match current() {
        Lang::Ru => "Тихие обновления включены.\n\nКогда BatteryHub запускается и находит новую версию, он скачивает её с GitHub, проверяет SHA-256 и устанавливает в фоне, без вопросов и окон. Настройки и автозапуск сохраняются.",
        Lang::En => "Silent updates are on.\n\nWhen BatteryHub starts and finds a new version, it downloads it from GitHub, checks its SHA-256 checksum and installs it in the background, without asking and without showing anything. Your settings and autostart are kept.",
    }
}
