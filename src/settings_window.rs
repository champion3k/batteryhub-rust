//! Settings window: device names, hidden devices, display order, thresholds, language, refresh,
//! autostart and Do not disturb. Modal: the caller reloads its settings after it closes.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::ffi::c_void;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetSysColorBrush, COLOR_WINDOW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::device::DeviceStatus;
use crate::i18n::{self, Lang};
use crate::install::{self, TASKBAR};
use crate::settings;
use crate::ui;

const CLASS: PCWSTR = w!("BatteryHubSettings");
const SETTINGS_KEY: &str = TASKBAR.settings_key;
const REFRESH: [u32; 5] = [1, 5, 10, 30, 60];

const IDC_LIST: i32 = 201;
const IDC_NAME: i32 = 202;
const IDC_RENAME: i32 = 203;
const IDC_HIDE: i32 = 204;
const IDC_UP: i32 = 205;
const IDC_DOWN: i32 = 206;
const IDC_THRESH: i32 = 207;
const IDC_AUTOSTART: i32 = 208;
const IDC_DND: i32 = 209;
const IDC_LANG: i32 = 210;
const IDC_REFRESH: i32 = 211;
const IDC_HISTORY: i32 = 212;
const IDC_CLOSE: i32 = 2;

const LBN_SELCHANGE: u32 = 1;
const CBN_SELCHANGE: u32 = 1;
const BN_CLICKED: u32 = 0;
const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_GETCURSEL: u32 = 0x0188;
const LB_SETCURSEL: u32 = 0x0186;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const BM_SETCHECK: u32 = 0x00F1;
const BM_GETCHECK: u32 = 0x00F0;
const BST_CHECKED: usize = 1;
const LBS_NOTIFY: u32 = 0x0001;
const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_AUTOCHECKBOX: u32 = 0x0003;

struct State {
    wnd: HWND,
    devices: Vec<DeviceStatus>,
    order: Vec<String>,
    aliases: BTreeMap<String, String>,
    hidden: HashSet<String>,
    name_edit: HWND,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn item(id: i32) -> HWND {
    unsafe { GetDlgItem(Some(wnd()), id).unwrap_or_default() }
}

fn wnd() -> HWND {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.wnd).unwrap_or_default())
}

fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(h, msg, Some(WPARAM(wp)), Some(LPARAM(lp))).0 }
}

fn sc(dpi: u32, v: i32) -> i32 {
    v * dpi as i32 / 96
}

fn add(class: PCWSTR, text: &str, style: u32, ex: u32, id: i32, x: i32, y: i32, w: i32, h: i32, dpi: u32) -> HWND {
    let hinst = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex),
            class,
            &HSTRING::from(text),
            WINDOW_STYLE(style),
            sc(dpi, x),
            sc(dpi, y),
            sc(dpi, w),
            sc(dpi, h),
            Some(wnd()),
            Some(HMENU(id as *mut c_void)),
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default()
    }
}

fn text_of_handle(h: HWND) -> String {
    let mut buf = vec![0u16; 512];
    let n = send(h, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize) as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn name_text() -> String {
    STATE.with(|s| s.borrow().as_ref().map(|s| text_of_handle(s.name_edit)).unwrap_or_default())
}

fn set_name_text(text: &str) {
    if let Some(h) = STATE.with(|s| s.borrow().as_ref().map(|s| s.name_edit)) {
        unsafe {
            let _ = SetWindowTextW(h, &HSTRING::from(text));
        }
    }
}

fn set_text(id: i32, text: &str) {
    unsafe {
        let _ = SetWindowTextW(item(id), &HSTRING::from(text));
    }
}

fn checked(id: i32) -> bool {
    send(item(id), BM_GETCHECK, 0, 0) as usize == BST_CHECKED
}

fn set_checked(id: i32, on: bool) {
    send(item(id), BM_SETCHECK, if on { 1 } else { 0 }, 0);
}

fn selected_key() -> Option<String> {
    let i = send(item(IDC_LIST), LB_GETCURSEL, 0, 0);
    if i < 0 {
        return None;
    }
    STATE.with(|s| s.borrow().as_ref().and_then(|s| s.order.get(i as usize).cloned()))
}

fn set_dword(name: &str, value: u32) {
    if let Some(k) = install::create(install::HKCU, SETTINGS_KEY) {
        let _ = k.set_dword(name, value);
    }
}

fn get_dword(name: &str) -> u32 {
    crate::util::reg_get_dword(SETTINGS_KEY, name).unwrap_or(0)
}

fn autostart_name() -> &'static str {
    TASKBAR.run_value.unwrap_or("BatteryHub Taskbar")
}

fn current_device() -> Option<DeviceStatus> {
    let key = selected_key()?;
    STATE.with(|s| s.borrow().as_ref().and_then(|s| s.devices.iter().find(|d| d.key == key).cloned()))
}

fn refresh_list(select: Option<String>) {
    let list = item(IDC_LIST);
    send(list, LB_RESETCONTENT, 0, 0);
    let Some((keys, entries)) = STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let st = guard.as_mut()?;
        let mut keys = st.order.clone();
        for d in &st.devices {
            if !keys.contains(&d.key) {
                keys.push(d.key.clone());
            }
        }
        keys.retain(|k| st.devices.iter().any(|d| &d.key == k));
        st.order = keys.clone();
        let entries: Vec<String> = keys
            .iter()
            .filter_map(|k| {
                let d = st.devices.iter().find(|d| &d.key == k)?;
                let alias = st.aliases.get(&d.key).unwrap_or(&d.name);
                let pct = if d.percent >= 0 { format!("{}%", d.percent) } else { "—".into() };
                let mark = if st.hidden.contains(&d.key) { "○" } else { "●" };
                Some(format!("{mark}  {alias}   {pct}"))
            })
            .collect();
        Some((keys, entries))
    }) else {
        return;
    };
    if entries.is_empty() {
        let txt = i18n::settings().no_devices;
        send(list, LB_ADDSTRING, 0, HSTRING::from(txt).as_ptr() as isize);
        return;
    }
    for label in entries {
        send(list, LB_ADDSTRING, 0, HSTRING::from(label).as_ptr() as isize);
    }
    let i = select.as_ref().and_then(|k| keys.iter().position(|x| x == k)).unwrap_or(0);
    send(list, LB_SETCURSEL, i, 0);
    on_select();
}

fn on_select() {
    let Some(d) = current_device() else { return };
    let (alias, hidden) = STATE.with(|s| {
        let guard = s.borrow();
        let st = guard.as_ref();
        let alias = st.and_then(|s| s.aliases.get(&d.key).cloned()).unwrap_or_else(|| d.name.clone());
        let hidden = st.is_some_and(|s| s.hidden.contains(&d.key));
        (alias, hidden)
    });
    let threshold = settings::threshold(SETTINGS_KEY, &d.key);
    set_name_text(&alias);
    let i = settings::THRESHOLDS.iter().position(|t| *t == threshold).unwrap_or(4);
    send(item(IDC_THRESH), CB_SETCURSEL, i, 0);
    set_text(IDC_HIDE, if hidden { i18n::settings().show } else { i18n::settings().hide });
}

fn create_controls(h: HWND) {
    let dpi = unsafe { GetDpiForWindow(h) };
    let s = i18n::settings();
    let t = i18n::t();
    add(w!("STATIC"), s.devices, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 16, 16, 400, 18, dpi);
    add(
        w!("LISTBOX"),
        "",
        WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | WS_BORDER.0 | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        0,
        IDC_LIST,
        16,
        38,
        552,
        190,
        dpi,
    );
    add(w!("STATIC"), s.name, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 16, 238, 380, 18, dpi);
    let name_edit = add(w!("EDIT"), "", WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | ES_AUTOHSCROLL, 0, IDC_NAME, 16, 258, 380, 24, dpi);
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.name_edit = name_edit;
        }
    });
    add(w!("BUTTON"), s.rename, WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_RENAME, 404, 258, 164, 28, dpi);
    add(w!("BUTTON"), s.hide, WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_HIDE, 16, 294, 150, 28, dpi);
    add(w!("BUTTON"), "↑", WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_UP, 178, 294, 48, 28, dpi);
    add(w!("BUTTON"), "↓", WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_DOWN, 234, 294, 48, 28, dpi);
    add(w!("STATIC"), s.hint, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 16, 330, 552, 18, dpi);
    add(w!("STATIC"), s.threshold, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 16, 356, 220, 18, dpi);
    add(w!("COMBOBOX"), "", WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST, 0, IDC_THRESH, 16, 376, 220, 200, dpi);
    let auto = add(
        w!("BUTTON"),
        t.autostart,
        WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTOCHECKBOX,
        0,
        IDC_AUTOSTART,
        256,
        376,
        312,
        24,
        dpi,
    );
    set_checked(IDC_AUTOSTART, ui::autostart_enabled(autostart_name()));
    let _ = auto;
    add(w!("STATIC"), t.language, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 16, 412, 200, 18, dpi);
    add(w!("COMBOBOX"), "", WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST, 0, IDC_LANG, 16, 432, 220, 120, dpi);
    add(w!("STATIC"), s.refresh, WS_CHILD.0 | WS_VISIBLE.0, 0, -1, 256, 412, 200, 18, dpi);
    add(w!("COMBOBOX"), "", WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST, 0, IDC_REFRESH, 256, 432, 220, 200, dpi);
    add(w!("BUTTON"), t.dnd, WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTOCHECKBOX, 0, IDC_DND, 16, 468, 300, 24, dpi);
    add(w!("BUTTON"), t.history, WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_HISTORY, 16, 502, 210, 30, dpi);
    add(w!("BUTTON"), s.close, WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0, 0, IDC_CLOSE, 432, 502, 136, 30, dpi);

    // Thresholds.
    for t in settings::THRESHOLDS {
        send(item(IDC_THRESH), CB_ADDSTRING, 0, HSTRING::from(format!("{t}%")).as_ptr() as isize);
    }
    // Languages.
    send(item(IDC_LANG), CB_ADDSTRING, 0, HSTRING::from(Lang::En.native_name()).as_ptr() as isize);
    send(item(IDC_LANG), CB_ADDSTRING, 0, HSTRING::from(Lang::Ru.native_name()).as_ptr() as isize);
    send(item(IDC_LANG), CB_SETCURSEL, i18n::current() as usize, 0);
    // Refresh intervals.
    for m in REFRESH {
        send(item(IDC_REFRESH), CB_ADDSTRING, 0, HSTRING::from(i18n::interval(m)).as_ptr() as isize);
    }
    let current = get_dword("RefreshMinutes");
    let i = REFRESH.iter().position(|m| *m == current).unwrap_or(0);
    send(item(IDC_REFRESH), CB_SETCURSEL, i, 0);
    set_checked(IDC_DND, get_dword("DoNotDisturb") != 0);
    refresh_list(None);
}

fn on_command(id: i32, code: u32) {
    match id {
        IDC_LIST if code == LBN_SELCHANGE => on_select(),
        IDC_CLOSE => unsafe {
            let _ = DestroyWindow(wnd());
        },
        IDC_RENAME => {
            let Some(key) = selected_key() else { return };
            let alias = name_text().trim().to_string();
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    if alias.is_empty() {
                        s.aliases.remove(&key);
                    } else {
                        s.aliases.insert(key.clone(), alias);
                    }
                    settings::save_aliases(SETTINGS_KEY, &s.aliases);
                }
            });
            refresh_list(Some(key));
        }
        IDC_HIDE => {
            let Some(key) = selected_key() else { return };
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    if s.hidden.contains(&key) {
                        s.hidden.remove(&key);
                    } else {
                        s.hidden.insert(key.clone());
                    }
                    settings::save_hidden(SETTINGS_KEY, &s.hidden);
                }
            });
            refresh_list(Some(key));
        }
        IDC_UP | IDC_DOWN => {
            let Some(key) = selected_key() else { return };
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    if let Some(i) = s.order.iter().position(|k| *k == key) {
                        let j = if id == IDC_UP { i.saturating_sub(1) } else { (i + 1).min(s.order.len().saturating_sub(1)) };
                        s.order.swap(i, j);
                        settings::save_order(SETTINGS_KEY, &s.order);
                    }
                }
            });
            refresh_list(Some(key));
        }
        IDC_THRESH if code == CBN_SELCHANGE => {
            let Some(key) = selected_key() else { return };
            let i = send(item(IDC_THRESH), CB_GETCURSEL, 0, 0);
            if let Some(t) = settings::THRESHOLDS.get(i.max(0) as usize) {
                settings::set_threshold(SETTINGS_KEY, &key, *t);
            }
        }
        IDC_AUTOSTART if code == BN_CLICKED => {
            let on = checked(IDC_AUTOSTART);
            ui::set_autostart(autostart_name(), on);
            if on {
                crate::util::reg_delete_value(crate::install::STARTUP_APPROVED_KEY, autostart_name());
            }
        }
        IDC_DND if code == BN_CLICKED => set_dword("DoNotDisturb", checked(IDC_DND) as u32),
        IDC_LANG if code == CBN_SELCHANGE => {
            let i = send(item(IDC_LANG), CB_GETCURSEL, 0, 0);
            i18n::set(if i == 1 { Lang::Ru } else { Lang::En });
        }
        IDC_REFRESH if code == CBN_SELCHANGE => {
            let i = send(item(IDC_REFRESH), CB_GETCURSEL, 0, 0);
            if let Some(m) = REFRESH.get(i.max(0) as usize) {
                set_dword("RefreshMinutes", *m);
            }
        }
        IDC_HISTORY => {
            let _ = crate::history::open_report();
        }
        _ => {}
    }
}

extern "system" fn proc_(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                STATE.with(|s| {
                    *s.borrow_mut() = Some(State {
                        wnd: h,
                        devices: Vec::new(),
                        order: settings::load_order(SETTINGS_KEY),
                        aliases: settings::load_aliases(SETTINGS_KEY),
                        hidden: settings::load_hidden(SETTINGS_KEY),
                        name_edit: HWND::default(),
                    });
                });
                create_controls(h);
                LRESULT(0)
            }
            WM_COMMAND => {
                on_command((wp.0 & 0xFFFF) as i32, ((wp.0 >> 16) & 0xFFFF) as u32);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(h);
                LRESULT(0)
            }
            WM_DESTROY => {
                STATE.with(|s| *s.borrow_mut() = None);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }
}

/// Opens the modal settings window for the given devices. Call from the UI thread.
pub fn show(devices: Vec<DeviceStatus>, owner: HWND) {
    if !wnd().is_invalid() {
        unsafe {
            let _ = ShowWindow(wnd(), SW_RESTORE);
            let _ = SetForegroundWindow(wnd());
        }
        return;
    }
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem();
        let (w, h) = (sc(dpi, 584), sc(dpi, 610));
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let (x, y) = ((sw - w) / 2, (sh - h) / 2);
        let Ok(win) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            &HSTRING::from(i18n::settings().title),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            w,
            h,
            if owner.is_invalid() { None } else { Some(owner) },
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        // WM_CREATE filled the state with an empty device list: put the snapshot in before showing.
        STATE.with(|s| {
            if let Some(st) = s.borrow_mut().as_mut() {
                st.devices = devices;
            }
        });
        refresh_list(None);
        let _ = ShowWindow(win, SW_SHOW);
        let _ = SetForegroundWindow(win);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if IsDialogMessageW(win, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // The modal loop consumed WM_QUIT; the caller's own message loop continues.
    }
}
