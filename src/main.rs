#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod config;

#[cfg(windows)]
mod assets;
#[cfg(windows)]
mod editability;
#[cfg(windows)]
mod ime;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() {
    eprintln!("ime-cursor is a Windows desktop application. Build it on Windows.");
}

#[cfg(windows)]
fn main() {
    windows_app::run();
}

#[cfg(windows)]
mod windows_app {
    use crate::assets::*;
    use crate::config::{Config, ImeTargetMode};
    use crate::editability::{Editability, EditabilityDetector};
    use crate::ime::{root_window, window_at_cursor, ImeEngine, ImeSnapshot, Validity};
    use crate::win::*;
    use std::ffi::{c_void, OsStr};
    use std::iter::once;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    const APP_VERSION: &str = "1.0.4";
    const MAIN_CLASS: &str = "ImeCursorRust.MainWindow";
    const BADGE_CLASS: &str = "ImeCursorRust.BadgeWindow";
    const SETTINGS_CLASS: &str = "ImeCursorRust.SettingsWindow";
    const MUTEX_NAME: &str = "Local\\ImeCursorRust.Singleton.7740C7D2-5D89-4874-A4D5-1B344507A604";

    const TIMER_ID: usize = 1;
    const TRAY_ID: u32 = 1;
    const TIMER_INTERVAL_MS: u32 = 50;
    const UNKNOWN_CLEAR_DELAY: Duration = Duration::from_millis(300);
    const PERIODIC_CURSOR_REAPPLY: Duration = Duration::from_secs(10);
    const FAILED_CURSOR_RETRY: Duration = Duration::from_secs(1);

    const BADGE_WIDTH: i32 = 24;
    const BADGE_HEIGHT: i32 = 20;
    const BADGE_OFFSET: i32 = 18;

    const MENU_TOGGLE_SOUND: u16 = 1001;
    const MENU_SETTINGS: u16 = 1002;
    const MENU_ABOUT: u16 = 1003;
    const MENU_EXIT: u16 = 1004;

    const CTRL_IME_TARGET: u16 = 2101;
    const CTRL_SHOW_ENGLISH: u16 = 2102;
    const CTRL_SHOW_JAPANESE: u16 = 2103;
    const CTRL_SHOW_KOREAN: u16 = 2104;
    const CTRL_SHOW_BADGE: u16 = 2105;
    const CTRL_PLAY_ALL: u16 = 2106;
    const CTRL_PLAY_ENGLISH: u16 = 2107;
    const CTRL_PLAY_JAPANESE: u16 = 2108;
    const CTRL_PLAY_KOREAN: u16 = 2109;
    const CTRL_SHOW_TRAY_STATE: u16 = 2110;
    const CTRL_OK: u16 = IDOK;
    const CTRL_CANCEL: u16 = IDCANCEL;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ImeKind {
        English,
        JapaneseHiragana,
        JapaneseKatakana,
        Korean,
        Unsupported,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum CursorVariant {
        Plain,
        EnglishLower,
        EnglishUpper,
        JapaneseHiragana,
        JapaneseKatakana,
        Korean,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TrayDisplay {
        Default,
        English,
        Japanese,
        Korean,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum CurrentCursorClass {
        IBeam,
        KnownOther,
        Unknown,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ScreenEdge {
        Left,
        Top,
        Right,
        Bottom,
    }

    #[derive(Clone, Copy)]
    struct TrayMenuPlacement {
        anchor: POINT,
        flags: UINT,
        exclude: Option<RECT>,
    }

    struct IconSet {
        default: HICON,
        english: HICON,
        japanese: HICON,
        korean: HICON,
        owned: Vec<HICON>,
    }

    impl IconSet {
        unsafe fn create() -> Self {
            let mut owned = Vec::with_capacity(4);
            let mut create = |hex: &str| {
                let icon = create_icon_from_hex(hex);
                if !icon.is_null() {
                    owned.push(icon);
                }
                icon
            };

            let mut default = create(ICON_DEFAULT_HEX);
            let mut english = create(ICON_E_HEX);
            let mut japanese = create(ICON_J_HEX);
            let mut korean = create(ICON_K_HEX);

            if default.is_null() {
                default = LoadIconW(null_mut(), make_int_resource(IDI_APPLICATION));
            }
            if english.is_null() {
                english = default;
            }
            if japanese.is_null() {
                japanese = default;
            }
            if korean.is_null() {
                korean = default;
            }

            Self {
                default,
                english,
                japanese,
                korean,
                owned,
            }
        }

        unsafe fn destroy(&mut self) {
            for icon in self.owned.drain(..) {
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
            }
            self.default = null_mut();
            self.english = null_mut();
            self.japanese = null_mut();
            self.korean = null_mut();
        }

        fn for_display(&self, display: TrayDisplay) -> HICON {
            match display {
                TrayDisplay::Default => self.default,
                TrayDisplay::English => self.english,
                TrayDisplay::Japanese => self.japanese,
                TrayDisplay::Korean => self.korean,
            }
        }
    }

    struct AppState {
        hinstance: HINSTANCE,
        main_hwnd: HWND,
        badge_hwnd: HWND,
        settings_hwnd: HWND,
        mutex_handle: HANDLE,
        taskbar_created_message: u32,

        exe_dir: PathBuf,
        config_path: PathBuf,
        config: Config,
        ime_engine: ImeEngine,
        editability_detector: EditabilityDetector,
        icons: IconSet,

        tray_added: bool,
        tray_display: Option<TrayDisplay>,
        old_kind: Option<ImeKind>,
        old_caps: bool,
        old_shift: bool,
        last_cursor_apply: Option<Instant>,
        invalid_since: Option<Instant>,
        cleared_unknown: bool,
        cursor_apply_ok: bool,
        cursor_modified: bool,
        last_cursor_restore_attempt: Option<Instant>,
        was_text_cursor: bool,
        was_non_editable_text: bool,
        force_cursor_refresh: bool,

        badge_visible: bool,
        badge_kind: Option<ImeKind>,
        badge_text: Vec<u16>,
        badge_color: COLORREF,
        cleaning_up: bool,
    }

    impl AppState {
        fn new(
            hinstance: HINSTANCE,
            mutex_handle: HANDLE,
            taskbar_created_message: u32,
            exe_dir: PathBuf,
            config_path: PathBuf,
            config: Config,
            icons: IconSet,
        ) -> Self {
            Self {
                hinstance,
                main_hwnd: null_mut(),
                badge_hwnd: null_mut(),
                settings_hwnd: null_mut(),
                mutex_handle,
                taskbar_created_message,
                exe_dir,
                config_path,
                config,
                ime_engine: ImeEngine::default(),
                editability_detector: EditabilityDetector::new(),
                icons,
                tray_added: false,
                tray_display: None,
                old_kind: None,
                old_caps: false,
                old_shift: false,
                last_cursor_apply: None,
                invalid_since: None,
                cleared_unknown: false,
                cursor_apply_ok: false,
                cursor_modified: false,
                last_cursor_restore_attempt: None,
                was_text_cursor: false,
                was_non_editable_text: false,
                force_cursor_refresh: true,
                badge_visible: false,
                badge_kind: None,
                badge_text: wide_without_null("A"),
                badge_color: rgb(0x55, 0x55, 0x55),
                cleaning_up: false,
            }
        }

        unsafe fn initialize_window(&mut self, hwnd: HWND) {
            self.main_hwnd = hwnd;
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL, self.icons.default as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG, self.icons.default as LPARAM);

            self.badge_hwnd = create_badge_window(self);
            self.add_or_update_tray(true);
            SetTimer(hwnd, TIMER_ID, TIMER_INTERVAL_MS, None);
        }

        unsafe fn on_timer(&mut self) {
            // Do not query or display IME state while the pointer is using a normal,
            // link, resize, busy, or application-defined cursor. This keeps the
            // feature scoped to actual text-entry locations only.
            if current_cursor_class() != CurrentCursorClass::IBeam {
                self.hide_badge();

                // Force a fresh cursor update when the pointer next enters an I-Beam
                // area. Keep old_kind intact so merely moving in and out of a text
                // field does not replay the language-change sound.
                if self.was_text_cursor {
                    self.force_cursor_refresh = true;
                    self.invalid_since = None;
                    self.cleared_unknown = false;
                }
                self.was_text_cursor = false;
                self.was_non_editable_text = false;
                return;
            }

            let editability = self.editability_detector.at_cursor();
            if !editability.allows_custom_cursor() {
                self.hide_badge();
                let retry_restore = self.cursor_modified
                    && self.last_cursor_restore_attempt.map_or(true, |last| {
                        Instant::now()
                            .checked_duration_since(last)
                            .is_some_and(|elapsed| elapsed > FAILED_CURSOR_RETRY)
                    });
                if !self.was_non_editable_text || retry_restore {
                    self.restore_windows_cursor_scheme();
                }
                self.was_text_cursor = true;
                self.was_non_editable_text = true;
                self.force_cursor_refresh = true;
                self.invalid_since = None;
                self.cleared_unknown = false;
                return;
            }

            if !self.was_text_cursor || self.was_non_editable_text {
                self.force_cursor_refresh = true;
            }
            self.was_text_cursor = true;
            self.was_non_editable_text = false;

            let snapshot = self.ime_engine.query(self.config.ime_target_mode);
            self.show_ime(snapshot);
        }

        unsafe fn show_ime(&mut self, snapshot: ImeSnapshot) {
            let now = Instant::now();
            if snapshot.validity == Validity::Invalid {
                self.hide_badge();
                let invalid_since = self.invalid_since.get_or_insert(now);
                if !self.cleared_unknown
                    && now
                        .checked_duration_since(*invalid_since)
                        .is_some_and(|elapsed| elapsed > UNKNOWN_CLEAR_DELAY)
                {
                    self.apply_cursor(CursorVariant::Plain);
                    self.set_tray_display(TrayDisplay::Default, false);
                    self.cleared_unknown = true;
                    self.old_kind = None;
                }
                return;
            }

            self.invalid_since = None;
            self.cleared_unknown = false;

            let kind = classify_ime(snapshot);
            let caps = (GetKeyState(VK_CAPITAL) & 1) != 0;
            // Shift is a physical key state. GetKeyState is tied to the caller's
            // message queue, while this process owns only a hidden tray window.
            let shift = GetAsyncKeyState(VK_SHIFT) < 0;

            let elapsed_since_apply = self
                .last_cursor_apply
                .and_then(|last| now.checked_duration_since(last));
            let periodic_reapply = elapsed_since_apply
                .is_some_and(|elapsed| elapsed > PERIODIC_CURSOR_REAPPLY)
                && is_current_system_ibeam();
            let retry_failed_apply = !self.cursor_apply_ok
                && elapsed_since_apply
                    .map_or(true, |elapsed| elapsed > FAILED_CURSOR_RETRY);

            let need_apply = self.old_kind != Some(kind)
                || self.old_caps != caps
                || self.old_shift != shift
                || self.force_cursor_refresh
                || periodic_reapply
                || retry_failed_apply;

            if need_apply {
                let cursor_variant = match kind {
                    ImeKind::English if self.config.show_english_ibeam => {
                        if caps != shift {
                            CursorVariant::EnglishUpper
                        } else {
                            CursorVariant::EnglishLower
                        }
                    }
                    ImeKind::JapaneseHiragana if self.config.show_japanese_ibeam => {
                        CursorVariant::JapaneseHiragana
                    }
                    ImeKind::JapaneseKatakana if self.config.show_japanese_ibeam => {
                        CursorVariant::JapaneseKatakana
                    }
                    ImeKind::Korean if self.config.show_korean_ibeam => CursorVariant::Korean,
                    _ => CursorVariant::Plain,
                };
                self.apply_cursor(cursor_variant);

                let tray_display = match kind {
                    ImeKind::English => TrayDisplay::English,
                    ImeKind::JapaneseHiragana | ImeKind::JapaneseKatakana => {
                        TrayDisplay::Japanese
                    }
                    ImeKind::Korean => TrayDisplay::Korean,
                    ImeKind::Unsupported => TrayDisplay::Default,
                };
                self.set_tray_display(tray_display, false);

                if self.config.play_sounds && self.old_kind != Some(kind) {
                    self.play_kind_sound(kind);
                }

                self.old_kind = Some(kind);
                self.old_caps = caps;
                self.old_shift = shift;
                self.last_cursor_apply = Some(now);
                self.force_cursor_refresh = false;
            }

            self.update_badge(kind, snapshot.target);
        }

        /// Restores the user's actual Windows cursor scheme instead of drawing
        /// a custom IME I-Beam over read-only or ambiguously classified text.
        unsafe fn restore_windows_cursor_scheme(&mut self) -> bool {
            self.last_cursor_restore_attempt = Some(Instant::now());
            let restored = SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0) != FALSE;
            if restored {
                self.cursor_modified = false;
                self.cursor_apply_ok = false;
                self.last_cursor_apply = None;
            }
            restored
        }

        unsafe fn apply_cursor(&mut self, variant: CursorVariant) -> bool {
            let hex = match variant {
                CursorVariant::Plain => CURSOR_DEFAULT_HEX,
                CursorVariant::EnglishLower => CURSOR_EL_HEX,
                CursorVariant::EnglishUpper => CURSOR_EU_HEX,
                CursorVariant::JapaneseHiragana => CURSOR_JH_HEX,
                CursorVariant::JapaneseKatakana => CURSOR_JK_HEX,
                CursorVariant::Korean => CURSOR_K_HEX,
            };

            let Some(xor_mask) = decode_hex(hex) else {
                self.cursor_apply_ok = false;
                return false;
            };
            let and_mask = vec![0xffu8; xor_mask.len()];
            let cursor = CreateCursor(
                null_mut(),
                15,
                15,
                32,
                32,
                and_mask.as_ptr().cast(),
                xor_mask.as_ptr().cast(),
            );
            if cursor.is_null() {
                self.cursor_apply_ok = false;
                return false;
            }

            let ok = SetSystemCursor(cursor, OCR_IBEAM) != FALSE;
            // SetSystemCursor takes ownership and destroys the cursor on success.
            if !ok {
                DestroyCursor(cursor);
            }
            self.cursor_apply_ok = ok;
            if ok {
                self.cursor_modified = true;
                self.last_cursor_restore_attempt = None;
            }
            ok
        }

        unsafe fn play_kind_sound(&self, kind: ImeKind) {
            let (enabled, filename) = match kind {
                ImeKind::English => (self.config.play_english_sound, "IMEE.wav"),
                ImeKind::JapaneseHiragana | ImeKind::JapaneseKatakana => {
                    (self.config.play_japanese_sound, "IMEJ.wav")
                }
                ImeKind::Korean => (self.config.play_korean_sound, "IMEK.wav"),
                ImeKind::Unsupported => return,
            };
            if !enabled {
                return;
            }

            let path = self.exe_dir.join(filename);
            if !path.is_file() {
                return;
            }
            let sound = path_to_wide(&path);
            PlaySoundW(
                sound.as_ptr(),
                null_mut(),
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            );
        }

        unsafe fn update_badge(&mut self, kind: ImeKind, target: HWND) {
            if !self.config.show_fallback_badge
                || self.badge_hwnd.is_null()
                || !self.badge_enabled_for_kind(kind)
                || !self.should_show_fallback_badge(target)
            {
                self.hide_badge();
                return;
            }

            let (text, color) = match kind {
                ImeKind::Korean => ("한", rgb(0x25, 0x63, 0xeb)),
                ImeKind::JapaneseHiragana | ImeKind::JapaneseKatakana => {
                    ("J", rgb(0x7c, 0x3a, 0xed))
                }
                ImeKind::English => ("A", rgb(0x55, 0x55, 0x55)),
                ImeKind::Unsupported => {
                    self.hide_badge();
                    return;
                }
            };

            if self.badge_kind != Some(kind) {
                self.badge_kind = Some(kind);
                self.badge_text = wide_without_null(text);
                self.badge_color = color;
                InvalidateRect(self.badge_hwnd, null(), TRUE);
            }

            let mut point = POINT::default();
            if GetCursorPos(&mut point) == FALSE {
                self.hide_badge();
                return;
            }
            let mut x = point.x + BADGE_OFFSET;
            let mut y = point.y + BADGE_OFFSET;

            let virtual_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let virtual_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let virtual_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let virtual_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let right = virtual_x.saturating_add(virtual_width);
            let bottom = virtual_y.saturating_add(virtual_height);

            if x + BADGE_WIDTH > right {
                x = right - BADGE_WIDTH;
            }
            if y + BADGE_HEIGHT > bottom {
                y = bottom - BADGE_HEIGHT;
            }
            x = x.max(virtual_x);
            y = y.max(virtual_y);

            SetWindowPos(
                self.badge_hwnd,
                HWND_TOPMOST,
                x,
                y,
                BADGE_WIDTH,
                BADGE_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            self.badge_visible = true;
        }

        fn badge_enabled_for_kind(&self, kind: ImeKind) -> bool {
            match kind {
                ImeKind::English => self.config.show_english_ibeam,
                ImeKind::JapaneseHiragana | ImeKind::JapaneseKatakana => {
                    self.config.show_japanese_ibeam
                }
                ImeKind::Korean => self.config.show_korean_ibeam,
                ImeKind::Unsupported => false,
            }
        }

        unsafe fn should_show_fallback_badge(&self, target: HWND) -> bool {
            let mouse_window = window_at_cursor();
            if mouse_window.is_null() || mouse_window == self.badge_hwnd {
                return false;
            }

            let mouse_root = root_window(mouse_window);
            let target_root = root_window(target);
            if !mouse_root.is_null() && !target_root.is_null() && mouse_root != target_root {
                return false;
            }

            // The timer already limits processing to a visible system I-Beam.
            // Recheck here because the cursor can change between the timer query and
            // badge placement. Unknown/custom cursors are deliberately excluded.
            current_cursor_class() == CurrentCursorClass::IBeam && !self.cursor_apply_ok
        }

        unsafe fn hide_badge(&mut self) {
            if self.badge_visible && !self.badge_hwnd.is_null() {
                ShowWindow(self.badge_hwnd, SW_HIDE);
            }
            self.badge_visible = false;
        }

        unsafe fn add_or_update_tray(&mut self, force: bool) {
            let display = self.display_for_current_state();
            if !self.tray_added {
                let mut data = self.notify_icon_data(display);
                if Shell_NotifyIconW(NIM_ADD, &mut data) != FALSE {
                    self.tray_added = true;
                    self.tray_display = Some(display);

                    let mut version_data: NOTIFYICONDATAW = zeroed();
                    version_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                    version_data.hWnd = self.main_hwnd;
                    version_data.uID = TRAY_ID;
                    version_data.uTimeoutOrVersion = NOTIFYICON_VERSION_4;
                    Shell_NotifyIconW(NIM_SETVERSION, &mut version_data);
                }
                return;
            }

            if force || self.tray_display != Some(display) {
                let mut data = self.notify_icon_data(display);
                if Shell_NotifyIconW(NIM_MODIFY, &mut data) != FALSE {
                    self.tray_display = Some(display);
                }
            }
        }

        unsafe fn set_tray_display(&mut self, requested: TrayDisplay, force: bool) {
            let display = if self.config.show_ime_tray_icon {
                requested
            } else {
                TrayDisplay::Default
            };
            if !self.tray_added {
                self.add_or_update_tray(force);
                return;
            }
            if !force && self.tray_display == Some(display) {
                return;
            }
            let mut data = self.notify_icon_data(display);
            if Shell_NotifyIconW(NIM_MODIFY, &mut data) != FALSE {
                self.tray_display = Some(display);
            }
        }

        fn display_for_current_state(&self) -> TrayDisplay {
            if !self.config.show_ime_tray_icon {
                return TrayDisplay::Default;
            }
            match self.old_kind {
                Some(ImeKind::English) => TrayDisplay::English,
                Some(ImeKind::JapaneseHiragana | ImeKind::JapaneseKatakana) => {
                    TrayDisplay::Japanese
                }
                Some(ImeKind::Korean) => TrayDisplay::Korean,
                _ => TrayDisplay::Default,
            }
        }

        unsafe fn notify_icon_data(&self, display: TrayDisplay) -> NOTIFYICONDATAW {
            let mut data: NOTIFYICONDATAW = zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.main_hwnd;
            data.uID = TRAY_ID;
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            data.uCallbackMessage = WM_APP_TRAY;
            data.hIcon = self.icons.for_display(display);
            let tip = match display {
                TrayDisplay::Default => "IME Cursor",
                TrayDisplay::English => "IME Cursor - English",
                TrayDisplay::Japanese => "IME Cursor - 日本語",
                TrayDisplay::Korean => "IME Cursor - 한글",
            };
            copy_wide_to_fixed(&wide_without_null(tip), &mut data.szTip);
            data
        }

        unsafe fn delete_tray(&mut self) {
            if !self.tray_added || self.main_hwnd.is_null() {
                return;
            }
            let mut data: NOTIFYICONDATAW = zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.main_hwnd;
            data.uID = TRAY_ID;
            Shell_NotifyIconW(NIM_DELETE, &mut data);
            self.tray_added = false;
            self.tray_display = None;
        }

        unsafe fn tray_set_focus(&self) {
            if !self.tray_added {
                return;
            }
            let mut data: NOTIFYICONDATAW = zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.main_hwnd;
            data.uID = TRAY_ID;
            Shell_NotifyIconW(NIM_SETFOCUS, &mut data);
        }

        unsafe fn on_tray_message(&mut self, wparam: WPARAM, lparam: LPARAM) {
            let event = loword(lparam as usize) as u32;
            match event {
                // With NOTIFYICON_VERSION_4, mouse notification coordinates are
                // packed into wParam. WM_CONTEXTMENU can be keyboard-generated,
                // so its wParam is undefined and the icon rectangle is preferred.
                WM_RBUTTONUP => self.show_tray_menu(Some(point_from_message(wparam))),
                WM_CONTEXTMENU => self.show_tray_menu(None),
                WM_LBUTTONDBLCLK | NIN_KEYSELECT => self.toggle_sounds(),
                _ => {}
            }
        }

        unsafe fn tray_icon_rect(&self) -> Option<RECT> {
            if !self.tray_added || self.main_hwnd.is_null() {
                return None;
            }

            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as DWORD,
                hWnd: self.main_hwnd,
                uID: TRAY_ID,
                guidItem: GUID::default(),
            };
            let mut rect = RECT::default();
            if Shell_NotifyIconGetRect(&identifier, &mut rect) == S_OK && rect_is_valid(rect) {
                Some(rect)
            } else {
                None
            }
        }

        unsafe fn tray_menu_placement(
            &self,
            event_anchor: Option<POINT>,
        ) -> TrayMenuPlacement {
            let icon_rect = self.tray_icon_rect();

            let mut fallback = event_anchor.unwrap_or_default();
            if event_anchor.is_none() && GetCursorPos(&mut fallback) == FALSE {
                fallback = POINT::default();
            }

            let reference = icon_rect.map(rect_center).unwrap_or(fallback);
            let monitor = MonitorFromPoint(reference, MONITOR_DEFAULTTONEAREST);
            if !monitor.is_null() {
                let mut info: MONITORINFO = zeroed();
                info.cbSize = size_of::<MONITORINFO>() as DWORD;
                if GetMonitorInfoW(monitor, &mut info) != FALSE
                    && rect_is_valid(info.rcMonitor)
                {
                    return calculate_tray_menu_placement(
                        icon_rect,
                        fallback,
                        info.rcMonitor,
                        info.rcWork,
                    );
                }
            }

            // Monitor APIs are expected to succeed on supported Windows versions,
            // but the virtual desktop still gives a safe edge-aware fallback.
            let virtual_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let virtual_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let virtual_screen = RECT {
                left: virtual_left,
                top: virtual_top,
                right: virtual_left.saturating_add(GetSystemMetrics(SM_CXVIRTUALSCREEN)),
                bottom: virtual_top.saturating_add(GetSystemMetrics(SM_CYVIRTUALSCREEN)),
            };
            calculate_tray_menu_placement(icon_rect, fallback, virtual_screen, virtual_screen)
        }

        unsafe fn show_tray_menu(&mut self, event_anchor: Option<POINT>) {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }

            let sound_flags = MF_STRING
                | if self.config.play_sounds {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
            append_menu_text(menu, sound_flags, MENU_TOGGLE_SOUND as usize, "소리 재생");
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu_text(menu, MF_STRING, MENU_SETTINGS as usize, "설정");
            append_menu_text(menu, MF_STRING, MENU_ABOUT as usize, "정보");
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu_text(menu, MF_STRING, MENU_EXIT as usize, "종료");

            let placement = self.tray_menu_placement(event_anchor);
            let params = placement.exclude.map(|rc_exclude| TPMPARAMS {
                cbSize: size_of::<TPMPARAMS>() as UINT,
                rcExclude: rc_exclude,
            });
            let params_ptr = params
                .as_ref()
                .map_or(null(), |value| value as *const TPMPARAMS);

            // The taskbar is a topmost window. Temporarily placing the hidden
            // owner in the topmost band prevents the native popup menu from
            // being occluded after fullscreen, sleep, or monitor transitions.
            let zorder_flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            SetWindowPos(
                self.main_hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                zorder_flags,
            );
            SetForegroundWindow(self.main_hwnd);

            let command = TrackPopupMenuEx(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY | placement.flags,
                placement.anchor.x,
                placement.anchor.y,
                self.main_hwnd,
                params_ptr,
            );

            SetWindowPos(
                self.main_hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                zorder_flags,
            );
            DestroyMenu(menu);
            PostMessageW(self.main_hwnd, WM_NULL, 0, 0);
            self.tray_set_focus();

            if command > 0 {
                self.handle_command(command as u16);
            }
        }

        unsafe fn handle_command(&mut self, command: u16) {
            match command {
                MENU_TOGGLE_SOUND => self.toggle_sounds(),
                MENU_SETTINGS => self.show_settings(),
                MENU_ABOUT => self.show_about(),
                MENU_EXIT => {
                    if !self.main_hwnd.is_null() {
                        DestroyWindow(self.main_hwnd);
                    }
                }
                _ => {}
            }
        }

        unsafe fn toggle_sounds(&mut self) {
            self.config.play_sounds = !self.config.play_sounds;
            self.save_config();
        }

        unsafe fn show_settings(&mut self) {
            if !self.settings_hwnd.is_null() && IsWindow(self.settings_hwnd) != FALSE {
                ShowWindow(self.settings_hwnd, SW_SHOWNORMAL);
                SetForegroundWindow(self.settings_hwnd);
                return;
            }

            let class = wide(SETTINGS_CLASS);
            let title = wide("IME Cursor 설정");
            let hwnd = CreateWindowExW(
                WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
                class.as_ptr(),
                title.as_ptr(),
                WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                430,
                480,
                self.main_hwnd,
                null_mut(),
                self.hinstance,
                self as *mut Self as *const c_void,
            );
            if hwnd.is_null() {
                return;
            }
            self.settings_hwnd = hwnd;
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            SetForegroundWindow(hwnd);
        }

        unsafe fn apply_settings_from_window(&mut self, hwnd: HWND) {
            let combo = GetDlgItem(hwnd, CTRL_IME_TARGET as i32);
            let selection = if combo.is_null() {
                0
            } else {
                SendMessageW(combo, CB_GETCURSEL, 0, 0)
            };
            self.config.ime_target_mode = if selection == 1 {
                ImeTargetMode::MouseControl
            } else {
                ImeTargetMode::FocusedControl
            };
            self.config.show_english_ibeam = read_checkbox(hwnd, CTRL_SHOW_ENGLISH);
            self.config.show_japanese_ibeam = read_checkbox(hwnd, CTRL_SHOW_JAPANESE);
            self.config.show_korean_ibeam = read_checkbox(hwnd, CTRL_SHOW_KOREAN);
            self.config.show_fallback_badge = read_checkbox(hwnd, CTRL_SHOW_BADGE);
            self.config.play_sounds = read_checkbox(hwnd, CTRL_PLAY_ALL);
            self.config.play_english_sound = read_checkbox(hwnd, CTRL_PLAY_ENGLISH);
            self.config.play_japanese_sound = read_checkbox(hwnd, CTRL_PLAY_JAPANESE);
            self.config.play_korean_sound = read_checkbox(hwnd, CTRL_PLAY_KOREAN);
            self.config.show_ime_tray_icon = read_checkbox(hwnd, CTRL_SHOW_TRAY_STATE);
            self.save_config();

            if !self.config.show_fallback_badge {
                self.hide_badge();
            }
            self.old_kind = None;
            self.last_cursor_apply = None;
            self.force_cursor_refresh = true;
            self.add_or_update_tray(true);
        }

        unsafe fn save_config(&self) {
            if let Err(error) = self.config.save(&self.config_path) {
                let text = wide(&format!(
                    "설정 파일을 저장하지 못했습니다.\n\n{}\n\n{}",
                    self.config_path.display(),
                    error
                ));
                let title = wide("IME Cursor 설정 오류");
                MessageBoxW(
                    self.main_hwnd,
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }

        unsafe fn show_about(&self) {
            let text = wide(&format!(
                "IME Cursor Rust {APP_VERSION}\n\n한글/영문/일본어 IME 상태를 텍스트 커서와 마우스 옆 배지로 표시합니다.\n\n설정 파일: {}",
                self.config_path.display()
            ));
            let title = wide("IME Cursor 정보");
            MessageBoxW(
                self.main_hwnd,
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }

        unsafe fn cleanup(&mut self) {
            if self.cleaning_up {
                return;
            }
            self.cleaning_up = true;

            if !self.main_hwnd.is_null() {
                KillTimer(self.main_hwnd, TIMER_ID);
            }
            self.hide_badge();
            self.delete_tray();

            if !self.settings_hwnd.is_null() && IsWindow(self.settings_hwnd) != FALSE {
                let hwnd = self.settings_hwnd;
                self.settings_hwnd = null_mut();
                DestroyWindow(hwnd);
            }
            if !self.badge_hwnd.is_null() && IsWindow(self.badge_hwnd) != FALSE {
                let hwnd = self.badge_hwnd;
                self.badge_hwnd = null_mut();
                DestroyWindow(hwnd);
            }

            if self.cursor_modified {
                self.restore_windows_cursor_scheme();
            }
            self.icons.destroy();

            if !self.mutex_handle.is_null() {
                CloseHandle(self.mutex_handle);
                self.mutex_handle = null_mut();
            }
        }
    }

    fn install_panic_hook() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // A panic inside an extern "system" window procedure terminates the
            // process after the hook runs. Restore the user's cursor scheme first.
            unsafe {
                SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
            }
            previous(info);
        }));
    }

    pub fn run() {
        install_panic_hook();
        unsafe {
            SetProcessDPIAware();

            let mutex_name = wide(MUTEX_NAME);
            let mutex_handle = CreateMutexW(null(), TRUE, mutex_name.as_ptr());
            if mutex_handle.is_null() {
                show_fatal_error("프로그램 단일 실행 잠금을 만들 수 없습니다.");
                return;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let class = wide(MAIN_CLASS);
                let existing = FindWindowW(class.as_ptr(), null());
                if !existing.is_null() {
                    PostMessageW(existing, WM_APP_SHOW_SETTINGS, 0, 0);
                }
                CloseHandle(mutex_handle);
                return;
            }

            let hinstance = GetModuleHandleW(null());
            if hinstance.is_null() {
                CloseHandle(mutex_handle);
                show_fatal_error("Windows 모듈 핸들을 가져올 수 없습니다.");
                return;
            }

            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ime-cursor.exe"));
            let exe_dir = exe_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let config_path = exe_dir.join("IMECur.ini");
            let config = Config::load(&config_path);
            let mut icons = IconSet::create();

            if !register_window_classes(hinstance, icons.default) {
                icons.destroy();
                CloseHandle(mutex_handle);
                show_fatal_error("Windows 창 클래스를 등록할 수 없습니다.");
                return;
            }

            let taskbar_message = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            let state = Box::new(AppState::new(
                hinstance,
                mutex_handle,
                taskbar_message,
                exe_dir,
                config_path,
                config,
                icons,
            ));
            let state_ptr = Box::into_raw(state);

            let class = wide(MAIN_CLASS);
            let title = wide("IME Cursor Rust Hidden Window");
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                hinstance,
                state_ptr as *const c_void,
            );

            if hwnd.is_null() {
                let state = &mut *state_ptr;
                state.cleanup();
                drop(Box::from_raw(state_ptr));
                show_fatal_error("메인 창을 만들 수 없습니다.");
                return;
            }

            let mut message: MSG = zeroed();
            loop {
                let result = GetMessageW(&mut message, null_mut(), 0, 0);
                if result == -1 || result == 0 {
                    break;
                }
                let settings = (*state_ptr).settings_hwnd;
                if !settings.is_null()
                    && IsWindow(settings) != FALSE
                    && IsDialogMessageW(settings, &mut message) != FALSE
                {
                    continue;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            let state = &mut *state_ptr;
            state.cleanup();
            drop(Box::from_raw(state_ptr));
        }
    }

    unsafe fn register_window_classes(hinstance: HINSTANCE, app_icon: HICON) -> bool {
        let arrow = LoadCursorW(null_mut(), make_int_resource(IDC_ARROW));

        let main_name = wide(MAIN_CLASS);
        let main_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(main_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: app_icon,
            hCursor: arrow,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: main_name.as_ptr(),
        };
        if RegisterClassW(&main_class) == 0 {
            return false;
        }

        let badge_name = wide(BADGE_CLASS);
        let badge_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(badge_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: badge_name.as_ptr(),
        };
        if RegisterClassW(&badge_class) == 0 {
            return false;
        }

        let settings_name = wide(SETTINGS_CLASS);
        let settings_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(settings_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: app_icon,
            hCursor: arrow,
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: settings_name.as_ptr(),
        };
        RegisterClassW(&settings_class) != 0
    }

    unsafe extern "system" fn main_wnd_proc(
        hwnd: HWND,
        message: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            set_window_long_ptr(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return TRUE as LRESULT;
        }

        let state_ptr = get_window_long_ptr(hwnd, GWLP_USERDATA) as *mut AppState;
        if state_ptr.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = &mut *state_ptr;

        if message == state.taskbar_created_message && message != 0 {
            state.tray_added = false;
            state.tray_display = None;
            state.add_or_update_tray(true);
            return 0;
        }

        match message {
            WM_CREATE => {
                state.initialize_window(hwnd);
                0
            }
            WM_TIMER if wparam == TIMER_ID => {
                state.on_timer();
                0
            }
            WM_APP_TRAY => {
                state.on_tray_message(wparam, lparam);
                0
            }
            WM_APP_SHOW_SETTINGS => {
                state.show_settings();
                0
            }
            WM_COMMAND => {
                state.handle_command(loword(wparam));
                0
            }
            WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
                state.old_kind = None;
                state.last_cursor_apply = None;
                state.was_text_cursor = false;
                state.was_non_editable_text = false;
                state.force_cursor_refresh = true;
                0
            }
            WM_QUERYENDSESSION => TRUE as LRESULT,
            WM_ENDSESSION => {
                if wparam != 0 {
                    state.cleanup();
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                state.cleanup();
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe extern "system" fn badge_wnd_proc(
        hwnd: HWND,
        message: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            set_window_long_ptr(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return TRUE as LRESULT;
        }

        match message {
            WM_NCHITTEST => HTTRANSPARENT,
            WM_PAINT => {
                let state_ptr = get_window_long_ptr(hwnd, GWLP_USERDATA) as *mut AppState;
                let mut paint: PAINTSTRUCT = zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                if !dc.is_null() && !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    let mut rect = RECT::default();
                    GetClientRect(hwnd, &mut rect);
                    let brush = CreateSolidBrush(state.badge_color);
                    if !brush.is_null() {
                        FillRect(dc, &rect, brush);
                        DeleteObject(brush as HGDIOBJ);
                    }

                    SetBkMode(dc, TRANSPARENT);
                    SetTextColor(dc, rgb(0xff, 0xff, 0xff));
                    let font = GetStockObject(DEFAULT_GUI_FONT);
                    let old_font = if font.is_null() {
                        null_mut()
                    } else {
                        SelectObject(dc, font)
                    };
                    DrawTextW(
                        dc,
                        state.badge_text.as_mut_ptr(),
                        state.badge_text.len() as i32,
                        &mut rect,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    if !old_font.is_null() {
                        SelectObject(dc, old_font);
                    }
                }
                EndPaint(hwnd, &paint);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe extern "system" fn settings_wnd_proc(
        hwnd: HWND,
        message: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            set_window_long_ptr(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return TRUE as LRESULT;
        }

        let state_ptr = get_window_long_ptr(hwnd, GWLP_USERDATA) as *mut AppState;
        match message {
            WM_CREATE => {
                if !state_ptr.is_null() {
                    create_settings_controls(hwnd, &mut *state_ptr);
                }
                0
            }
            WM_COMMAND => {
                if state_ptr.is_null() {
                    return 0;
                }
                let control_id = loword(wparam);
                let notification = hiword(wparam);
                if notification == BN_CLICKED {
                    match control_id {
                        CTRL_OK => {
                            (&mut *state_ptr).apply_settings_from_window(hwnd);
                            DestroyWindow(hwnd);
                        }
                        CTRL_CANCEL => {
                            DestroyWindow(hwnd);
                        }
                        _ => {}
                    }
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_NCDESTROY => {
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if state.settings_hwnd == hwnd {
                        state.settings_hwnd = null_mut();
                    }
                }
                set_window_long_ptr(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn create_badge_window(state: &mut AppState) -> HWND {
        let class = wide(BADGE_CLASS);
        let title = wide("IME Cursor Badge");
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            BADGE_WIDTH,
            BADGE_HEIGHT,
            null_mut(),
            null_mut(),
            state.hinstance,
            state as *mut AppState as *const c_void,
        );
        if !hwnd.is_null() {
            SetLayeredWindowAttributes(hwnd, 0, 225, LWA_ALPHA);
            ShowWindow(hwnd, SW_HIDE);
        }
        hwnd
    }

    unsafe fn create_settings_controls(hwnd: HWND, state: &mut AppState) {
        let font = GetStockObject(DEFAULT_GUI_FONT);

        create_control(
            state,
            hwnd,
            "BUTTON",
            "설정",
            WS_CHILD | WS_VISIBLE | BS_GROUPBOX,
            18,
            14,
            380,
            370,
            0,
            font,
        );
        create_control(
            state,
            hwnd,
            "STATIC",
            "IME 상태 기준:",
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            34,
            42,
            340,
            20,
            0,
            font,
        );
        let combo = create_control(
            state,
            hwnd,
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST | CBS_HASSTRINGS,
            34,
            64,
            344,
            160,
            CTRL_IME_TARGET,
            font,
        );
        if !combo.is_null() {
            send_combo_string(combo, "활성 창의 포커스/캐럿 컨트롤");
            send_combo_string(combo, "마우스 커서 아래 컨트롤");
            let selection = match state.config.ime_target_mode {
                ImeTargetMode::FocusedControl => 0,
                ImeTargetMode::MouseControl => 1,
            };
            SendMessageW(combo, CB_SETCURSEL, selection, 0);
        }

        let checks = [
            (CTRL_SHOW_ENGLISH, "영문 I-Beam 표시", state.config.show_english_ibeam),
            (
                CTRL_SHOW_JAPANESE,
                "일본어 I-Beam 표시",
                state.config.show_japanese_ibeam,
            ),
            (CTRL_SHOW_KOREAN, "한글 I-Beam 표시", state.config.show_korean_ibeam),
            (
                CTRL_SHOW_BADGE,
                "호환 배지 표시(자체 커서를 쓰는 앱)",
                state.config.show_fallback_badge,
            ),
            (CTRL_PLAY_ALL, "상태 변경 소리 전체 사용", state.config.play_sounds),
            (
                CTRL_PLAY_ENGLISH,
                "영문 전환 소리 재생",
                state.config.play_english_sound,
            ),
            (
                CTRL_PLAY_JAPANESE,
                "일본어 전환 소리 재생",
                state.config.play_japanese_sound,
            ),
            (
                CTRL_PLAY_KOREAN,
                "한글 전환 소리 재생",
                state.config.play_korean_sound,
            ),
            (
                CTRL_SHOW_TRAY_STATE,
                "IME 상태를 트레이 아이콘에 표시",
                state.config.show_ime_tray_icon,
            ),
        ];

        let mut y = 102;
        for (id, text, checked) in checks {
            let control = create_control(
                state,
                hwnd,
                "BUTTON",
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
                34,
                y,
                344,
                22,
                id,
                font,
            );
            if !control.is_null() {
                SendMessageW(
                    control,
                    BM_SETCHECK,
                    if checked { BST_CHECKED } else { BST_UNCHECKED },
                    0,
                );
            }
            y += 29;
        }

        create_control(
            state,
            hwnd,
            "BUTTON",
            "확인",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON,
            230,
            401,
            78,
            28,
            CTRL_OK,
            font,
        );
        create_control(
            state,
            hwnd,
            "BUTTON",
            "취소",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            318,
            401,
            78,
            28,
            CTRL_CANCEL,
            font,
        );
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn create_control(
        state: &AppState,
        parent: HWND,
        class_name: &str,
        text: &str,
        style: DWORD,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        id: u16,
        font: HGDIOBJ,
    ) -> HWND {
        let class = wide(class_name);
        let label = wide(text);
        let control = CreateWindowExW(
            0,
            class.as_ptr(),
            label.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            control_id(id),
            state.hinstance,
            null(),
        );
        if !control.is_null() && !font.is_null() {
            SendMessageW(control, WM_SETFONT, font as WPARAM, TRUE as LPARAM);
        }
        control
    }

    unsafe fn send_combo_string(combo: HWND, text: &str) {
        let value = wide(text);
        SendMessageW(combo, CB_ADDSTRING, 0, value.as_ptr() as LPARAM);
    }

    unsafe fn read_checkbox(parent: HWND, id: u16) -> bool {
        let control = GetDlgItem(parent, id as i32);
        !control.is_null() && SendMessageW(control, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT
    }

    fn classify_ime(snapshot: ImeSnapshot) -> ImeKind {
        let primary_language = snapshot.language_id & 0x03ff;
        if !snapshot.is_open {
            return ImeKind::English;
        }

        match primary_language {
            0x12 => {
                if snapshot.conversion_mode & 1 != 0 {
                    ImeKind::Korean
                } else {
                    ImeKind::English
                }
            }
            0x11 => match snapshot.conversion_mode {
                9 | 25 => ImeKind::JapaneseHiragana,
                3 | 11 | 19 | 27 => ImeKind::JapaneseKatakana,
                _ => ImeKind::English,
            },
            0x04 => ImeKind::Unsupported,
            _ => match snapshot.conversion_mode {
                9 | 25 => ImeKind::JapaneseHiragana,
                3 | 11 | 19 | 27 => ImeKind::JapaneseKatakana,
                mode if mode & 1 != 0 => ImeKind::Korean,
                _ => ImeKind::English,
            },
        }
    }

    unsafe fn current_cursor_class() -> CurrentCursorClass {
        let mut info: CURSORINFO = zeroed();
        info.cbSize = size_of::<CURSORINFO>() as u32;
        if GetCursorInfo(&mut info) == FALSE || info.flags & CURSOR_SHOWING == 0 || info.hCursor.is_null() {
            return CurrentCursorClass::Unknown;
        }

        let current = info.hCursor;
        if current == LoadCursorW(null_mut(), make_int_resource(IDC_IBEAM)) {
            return CurrentCursorClass::IBeam;
        }

        let known = [
            IDC_ARROW,
            IDC_WAIT,
            IDC_CROSS,
            IDC_UPARROW,
            IDC_SIZENWSE,
            IDC_SIZENESW,
            IDC_SIZEWE,
            IDC_SIZENS,
            IDC_SIZEALL,
            IDC_NO,
            IDC_HAND,
            IDC_APPSTARTING,
            IDC_HELP,
        ];
        if known
            .iter()
            .any(|id| current == LoadCursorW(null_mut(), make_int_resource(*id)))
        {
            CurrentCursorClass::KnownOther
        } else {
            CurrentCursorClass::Unknown
        }
    }

    unsafe fn is_current_system_ibeam() -> bool {
        current_cursor_class() == CurrentCursorClass::IBeam
    }

    unsafe fn create_icon_from_hex(hex: &str) -> HICON {
        let Some(bytes) = decode_hex(hex) else {
            return null_mut();
        };
        CreateIconFromResourceEx(
            bytes.as_ptr(),
            bytes.len() as u32,
            TRUE,
            0x0003_0000,
            16,
            16,
            0,
        )
    }

    fn decode_hex(hex: &str) -> Option<Vec<u8>> {
        if hex.len() % 2 != 0 {
            return None;
        }
        let bytes = hex.as_bytes();
        let mut output = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            output.push((high << 4) | low);
        }
        Some(output)
    }

    fn hex_nibble(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(once(0)).collect()
    }

    fn wide_without_null(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().collect()
    }

    fn path_to_wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(once(0)).collect()
    }

    fn copy_wide_to_fixed<const N: usize>(source: &[u16], destination: &mut [u16; N]) {
        destination.fill(0);
        let count = source.len().min(N.saturating_sub(1));
        destination[..count].copy_from_slice(&source[..count]);
    }

    fn point_from_message(value: WPARAM) -> POINT {
        POINT {
            x: loword(value) as i16 as i32,
            y: hiword(value) as i16 as i32,
        }
    }

    fn rect_is_valid(rect: RECT) -> bool {
        rect.right > rect.left && rect.bottom > rect.top
    }

    fn rect_center(rect: RECT) -> POINT {
        POINT {
            x: rect.left + (rect.right - rect.left) / 2,
            y: rect.top + (rect.bottom - rect.top) / 2,
        }
    }

    fn clamp_coordinate(value: i32, minimum: i32, maximum: i32) -> i32 {
        if maximum < minimum {
            minimum
        } else {
            value.max(minimum).min(maximum)
        }
    }

    fn nearest_screen_edge(point: POINT, monitor: RECT) -> ScreenEdge {
        let left = (point.x as i64 - monitor.left as i64).abs();
        let top = (point.y as i64 - monitor.top as i64).abs();
        let right = (monitor.right as i64 - point.x as i64).abs();
        let bottom = (monitor.bottom as i64 - point.y as i64).abs();

        // Bottom wins ties because the Windows taskbar defaults to that edge.
        let mut best = (ScreenEdge::Bottom, bottom);
        for candidate in [
            (ScreenEdge::Top, top),
            (ScreenEdge::Left, left),
            (ScreenEdge::Right, right),
        ] {
            if candidate.1 < best.1 {
                best = candidate;
            }
        }
        best.0
    }

    fn tray_screen_edge(reference: POINT, monitor: RECT, work: RECT) -> ScreenEdge {
        if work.bottom < monitor.bottom && reference.y >= work.bottom {
            ScreenEdge::Bottom
        } else if work.top > monitor.top && reference.y < work.top {
            ScreenEdge::Top
        } else if work.left > monitor.left && reference.x < work.left {
            ScreenEdge::Left
        } else if work.right < monitor.right && reference.x >= work.right {
            ScreenEdge::Right
        } else {
            nearest_screen_edge(reference, monitor)
        }
    }

    fn taskbar_or_icon_exclusion(
        edge: ScreenEdge,
        icon: RECT,
        monitor: RECT,
        work: RECT,
    ) -> RECT {
        let center = rect_center(icon);
        match edge {
            ScreenEdge::Bottom
                if work.bottom < monitor.bottom && center.y >= work.bottom =>
            {
                RECT {
                    left: monitor.left,
                    top: work.bottom,
                    right: monitor.right,
                    bottom: monitor.bottom,
                }
            }
            ScreenEdge::Top if work.top > monitor.top && center.y < work.top => RECT {
                left: monitor.left,
                top: monitor.top,
                right: monitor.right,
                bottom: work.top,
            },
            ScreenEdge::Left if work.left > monitor.left && center.x < work.left => RECT {
                left: monitor.left,
                top: monitor.top,
                right: work.left,
                bottom: monitor.bottom,
            },
            ScreenEdge::Right
                if work.right < monitor.right && center.x >= work.right =>
            {
                RECT {
                    left: work.right,
                    top: monitor.top,
                    right: monitor.right,
                    bottom: monitor.bottom,
                }
            }
            _ => icon,
        }
    }

    fn calculate_tray_menu_placement(
        icon_rect: Option<RECT>,
        fallback: POINT,
        monitor: RECT,
        work: RECT,
    ) -> TrayMenuPlacement {
        let work = if rect_is_valid(work) { work } else { monitor };
        let reference = icon_rect.map(rect_center).unwrap_or(fallback);
        let edge = tray_screen_edge(reference, monitor, work);
        let middle_x = work.left + (work.right - work.left) / 2;
        let middle_y = work.top + (work.bottom - work.top) / 2;

        let (anchor, flags) = match edge {
            ScreenEdge::Bottom => {
                let align_right = reference.x >= middle_x;
                let x = if let Some(icon) = icon_rect {
                    if align_right {
                        clamp_coordinate(icon.right, work.left, work.right)
                    } else {
                        clamp_coordinate(icon.left, work.left, work.right)
                    }
                } else {
                    clamp_coordinate(reference.x, work.left, work.right)
                };
                let y = icon_rect
                    .map(|icon| icon.top.min(work.bottom))
                    .unwrap_or(work.bottom);
                (
                    POINT {
                        x,
                        y: clamp_coordinate(y, work.top, work.bottom),
                    },
                    (if align_right {
                        TPM_RIGHTALIGN
                    } else {
                        TPM_LEFTALIGN
                    }) | TPM_BOTTOMALIGN,
                )
            }
            ScreenEdge::Top => {
                let align_right = reference.x >= middle_x;
                let x = if let Some(icon) = icon_rect {
                    if align_right {
                        clamp_coordinate(icon.right, work.left, work.right)
                    } else {
                        clamp_coordinate(icon.left, work.left, work.right)
                    }
                } else {
                    clamp_coordinate(reference.x, work.left, work.right)
                };
                let y = icon_rect
                    .map(|icon| icon.bottom.max(work.top))
                    .unwrap_or(work.top);
                (
                    POINT {
                        x,
                        y: clamp_coordinate(y, work.top, work.bottom),
                    },
                    if align_right {
                        TPM_RIGHTALIGN | TPM_TOPALIGN
                    } else {
                        TPM_LEFTALIGN | TPM_TOPALIGN
                    },
                )
            }
            ScreenEdge::Right => {
                let align_bottom = reference.y >= middle_y;
                let x = icon_rect
                    .map(|icon| icon.left.min(work.right))
                    .unwrap_or(work.right);
                let y = if let Some(icon) = icon_rect {
                    if align_bottom {
                        clamp_coordinate(icon.bottom, work.top, work.bottom)
                    } else {
                        clamp_coordinate(icon.top, work.top, work.bottom)
                    }
                } else {
                    clamp_coordinate(reference.y, work.top, work.bottom)
                };
                (
                    POINT {
                        x: clamp_coordinate(x, work.left, work.right),
                        y,
                    },
                    TPM_RIGHTALIGN
                        | (if align_bottom {
                            TPM_BOTTOMALIGN
                        } else {
                            TPM_TOPALIGN
                        }),
                )
            }
            ScreenEdge::Left => {
                let align_bottom = reference.y >= middle_y;
                let x = icon_rect
                    .map(|icon| icon.right.max(work.left))
                    .unwrap_or(work.left);
                let y = if let Some(icon) = icon_rect {
                    if align_bottom {
                        clamp_coordinate(icon.bottom, work.top, work.bottom)
                    } else {
                        clamp_coordinate(icon.top, work.top, work.bottom)
                    }
                } else {
                    clamp_coordinate(reference.y, work.top, work.bottom)
                };
                (
                    POINT {
                        x: clamp_coordinate(x, work.left, work.right),
                        y,
                    },
                    TPM_LEFTALIGN
                        | (if align_bottom {
                            TPM_BOTTOMALIGN
                        } else {
                            TPM_TOPALIGN
                        }),
                )
            }
        };

        TrayMenuPlacement {
            anchor,
            flags,
            exclude: icon_rect
                .map(|icon| taskbar_or_icon_exclusion(edge, icon, monitor, work)),
        }
    }

    unsafe fn append_menu_text(menu: HMENU, flags: UINT, id: usize, text: &str) {
        let label = wide(text);
        AppendMenuW(menu, flags, id, label.as_ptr());
    }

    unsafe fn show_fatal_error(message: &str) {
        let text = wide(message);
        let title = wide("IME Cursor Rust 오류");
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn embedded_assets_have_expected_sizes() {
            for cursor in [
                CURSOR_DEFAULT_HEX,
                CURSOR_EL_HEX,
                CURSOR_EU_HEX,
                CURSOR_JH_HEX,
                CURSOR_JK_HEX,
                CURSOR_K_HEX,
            ] {
                assert_eq!(decode_hex(cursor).map(|v| v.len()), Some(128));
            }
            for icon in [ICON_DEFAULT_HEX, ICON_E_HEX, ICON_J_HEX, ICON_K_HEX] {
                assert_eq!(decode_hex(icon).map(|v| v.len()), Some(296));
            }
        }

        #[test]
        fn tray_menu_uses_work_area_above_bottom_taskbar() {
            let monitor = RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            };
            let work = RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            };
            let icon = RECT {
                left: 1850,
                top: 1048,
                right: 1874,
                bottom: 1072,
            };
            let placement = calculate_tray_menu_placement(
                Some(icon),
                POINT::default(),
                monitor,
                work,
            );

            assert_eq!(placement.anchor.y, work.bottom);
            assert_ne!(placement.flags & TPM_BOTTOMALIGN, 0);
            assert_ne!(placement.flags & TPM_RIGHTALIGN, 0);
            let exclude = placement.exclude.expect("taskbar exclusion");
            assert_eq!(exclude.top, work.bottom);
            assert_eq!(exclude.bottom, monitor.bottom);
        }

        #[test]
        fn tray_menu_supports_top_and_side_taskbars() {
            let monitor = RECT {
                left: -1600,
                top: 0,
                right: 0,
                bottom: 900,
            };

            let top_work = RECT {
                left: -1600,
                top: 48,
                right: 0,
                bottom: 900,
            };
            let top_icon = RECT {
                left: -80,
                top: 10,
                right: -56,
                bottom: 34,
            };
            let top = calculate_tray_menu_placement(
                Some(top_icon),
                POINT::default(),
                monitor,
                top_work,
            );
            assert_eq!(top.anchor.y, top_work.top);
            assert_eq!(top.flags & TPM_BOTTOMALIGN, 0);

            let right_work = RECT {
                left: -1600,
                top: 0,
                right: -52,
                bottom: 900,
            };
            let right_icon = RECT {
                left: -42,
                top: 830,
                right: -18,
                bottom: 854,
            };
            let right = calculate_tray_menu_placement(
                Some(right_icon),
                POINT::default(),
                monitor,
                right_work,
            );
            assert_eq!(right.anchor.x, right_work.right);
            assert_ne!(right.flags & TPM_RIGHTALIGN, 0);
            assert_ne!(right.flags & TPM_BOTTOMALIGN, 0);
        }

        #[test]
        fn notification_coordinates_keep_negative_monitor_values() {
            let packed = ((-120i16 as u16 as usize) << 16) | (-640i16 as u16 as usize);
            let point = point_from_message(packed);
            assert_eq!(point.x, -640);
            assert_eq!(point.y, -120);
        }
    }
}
