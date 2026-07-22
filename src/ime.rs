use crate::config::ImeTargetMode;
use crate::win::*;
use std::collections::{HashMap, HashSet};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

const CACHE_BRIDGE: Duration = Duration::from_millis(400);
const CACHE_RETENTION: Duration = Duration::from_secs(10);

const LANG_CHINESE: u16 = 0x04;
const LANG_JAPANESE: u16 = 0x11;
const LANG_KOREAN: u16 = 0x12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Validity {
    Invalid,
    Live,
    Cached,
}

#[derive(Clone, Copy, Debug)]
pub struct ImeSnapshot {
    pub validity: Validity,
    pub conversion_mode: u32,
    pub target: HWND,
    pub target_thread: u32,
    pub language_id: u16,
    pub is_open: bool,
}

impl Default for ImeSnapshot {
    fn default() -> Self {
        Self {
            validity: Validity::Invalid,
            conversion_mode: 0,
            target: null_mut(),
            target_thread: 0,
            language_id: 0,
            is_open: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CacheKey {
    Thread(u32),
    Window(usize),
}

#[derive(Clone, Copy, Debug)]
struct CachedIme {
    mode: u32,
    is_open: bool,
    language_id: u16,
    tick: Instant,
}

#[derive(Clone, Copy, Default)]
struct ForegroundTargets {
    active: HWND,
    focus: HWND,
    caret: HWND,
    foreground: HWND,
    thread_id: u32,
}

#[derive(Default)]
pub struct ImeEngine {
    cache: HashMap<CacheKey, CachedIme>,
}

impl ImeEngine {
    pub fn query(&mut self, target_mode: ImeTargetMode) -> ImeSnapshot {
        unsafe { self.query_unsafe(target_mode) }
    }

    unsafe fn query_unsafe(&mut self, target_mode: ImeTargetMode) -> ImeSnapshot {
        let now = Instant::now();
        if self.cache.len() > 128 {
            self.cache.retain(|_, value| {
                now.checked_duration_since(value.tick)
                    .is_some_and(|age| age <= CACHE_RETENTION)
            });
        }

        let foreground = foreground_targets();
        let mut candidates = Vec::<HWND>::with_capacity(24);
        let mut seen = HashSet::<usize>::with_capacity(24);
        let mut target = null_mut();

        match target_mode {
            ImeTargetMode::MouseControl => {
                let mouse = window_at_cursor();
                target = mouse;
                let root_mouse = root_window(mouse);
                let focused = if !foreground.focus.is_null() {
                    foreground.focus
                } else {
                    foreground.caret
                };
                let root_focus = root_window(focused);

                // When the pointer is still inside the active application, the focused/caret
                // control is usually the true owner of IME state even when the app draws a
                // custom editor surface under the mouse.
                if !root_mouse.is_null() && root_mouse == root_focus {
                    add_window_chain(&mut candidates, &mut seen, foreground.focus);
                    add_window_chain(&mut candidates, &mut seen, foreground.caret);
                }
                add_window_chain(&mut candidates, &mut seen, mouse);
                add_window_chain(&mut candidates, &mut seen, root_mouse);
            }
            ImeTargetMode::FocusedControl => {
                target = first_non_null(&[
                    foreground.focus,
                    foreground.caret,
                    foreground.active,
                    foreground.foreground,
                ]);
                add_window_chain(&mut candidates, &mut seen, foreground.focus);
                add_window_chain(&mut candidates, &mut seen, foreground.caret);
                add_window_chain(&mut candidates, &mut seen, foreground.active);
                add_window_chain(&mut candidates, &mut seen, foreground.foreground);
            }
        }

        if target.is_null() {
            target = candidates.first().copied().unwrap_or(null_mut());
        }
        if target.is_null() {
            return ImeSnapshot::default();
        }

        let mut first_thread = 0u32;
        let mut queried_ime_windows = HashSet::<usize>::with_capacity(candidates.len());

        for &hwnd in &candidates {
            let thread_id = window_thread(hwnd);
            if first_thread == 0 && thread_id != 0 {
                first_thread = thread_id;
            }

            let ime_window = ImmGetDefaultIMEWnd(hwnd);
            if ime_window.is_null() || !queried_ime_windows.insert(ime_window as usize) {
                continue;
            }

            if let Some(query) = query_ime_window(ime_window) {
                let language_id = thread_language(thread_id);
                let primary_language = language_id & 0x03ff;
                let mut mode = query.conversion_mode;

                // Some modern IMEs answer the open-status request but do not answer the
                // conversion-mode request. Do not misclassify that failure as English.
                if query.is_open && !query.conversion_valid {
                    mode = match primary_language {
                        LANG_JAPANESE => 9,
                        LANG_KOREAN => 1,
                        _ => 0,
                    };
                }

                let key = cache_key(thread_id, hwnd);
                self.cache.insert(
                    key,
                    CachedIme {
                        mode,
                        is_open: query.is_open,
                        language_id,
                        tick: now,
                    },
                );

                return ImeSnapshot {
                    validity: Validity::Live,
                    conversion_mode: mode,
                    target,
                    target_thread: thread_id,
                    language_id,
                    is_open: query.is_open,
                };
            }
        }

        let mut target_thread = window_thread(target);
        if target_thread == 0 {
            target_thread = first_thread;
        }
        let mut language_id = thread_language(target_thread);

        // Briefly reuse a state from the same UI thread while focus is moving between
        // controls. The short lifetime prevents a stale Korean/Japanese state from being
        // displayed indefinitely after the target application disappears.
        for &hwnd in &candidates {
            let thread_id = window_thread(hwnd);
            let key = cache_key(thread_id, hwnd);
            let Some(cached) = self.cache.get(&key).copied() else {
                continue;
            };
            let Some(age) = now.checked_duration_since(cached.tick) else {
                continue;
            };
            if age <= CACHE_BRIDGE {
                target_thread = thread_id;
                language_id = cached.language_id;
                return ImeSnapshot {
                    validity: Validity::Cached,
                    conversion_mode: cached.mode,
                    target,
                    target_thread,
                    language_id,
                    is_open: cached.is_open,
                };
            }
        }

        let primary_language = language_id & 0x03ff;
        if language_id != 0
            && primary_language != LANG_CHINESE
            && primary_language != LANG_JAPANESE
            && primary_language != LANG_KOREAN
        {
            return ImeSnapshot {
                validity: Validity::Live,
                conversion_mode: 0,
                target,
                target_thread,
                language_id,
                is_open: false,
            };
        }

        ImeSnapshot {
            validity: Validity::Invalid,
            conversion_mode: 0,
            target,
            target_thread,
            language_id,
            is_open: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ImeWindowQuery {
    is_open: bool,
    conversion_mode: u32,
    conversion_valid: bool,
}

unsafe fn query_ime_window(ime_window: HWND) -> Option<ImeWindowQuery> {
    if ime_window.is_null() || IsWindow(ime_window) == FALSE {
        return None;
    }

    let open_result = send_ime_control(ime_window, IMC_GETOPENSTATUS)?;
    let is_open = open_result != 0;
    if !is_open {
        return Some(ImeWindowQuery {
            is_open: false,
            conversion_mode: 0,
            conversion_valid: false,
        });
    }

    match send_ime_control(ime_window, IMC_GETCONVERSIONMODE) {
        Some(mode) => Some(ImeWindowQuery {
            is_open: true,
            conversion_mode: mode as u32,
            conversion_valid: true,
        }),
        None => Some(ImeWindowQuery {
            is_open: true,
            conversion_mode: 0,
            conversion_valid: false,
        }),
    }
}

unsafe fn send_ime_control(ime_window: HWND, command: WPARAM) -> Option<usize> {
    let mut result = 0usize;
    let ok = SendMessageTimeoutW(
        ime_window,
        WM_IME_CONTROL,
        command,
        0,
        SMTO_BLOCK | SMTO_ABORTIFHUNG,
        50,
        &mut result,
    );
    (ok != 0).then_some(result)
}

unsafe fn foreground_targets() -> ForegroundTargets {
    let foreground = GetForegroundWindow();
    let thread_id = window_thread(foreground);
    let mut result = ForegroundTargets {
        active: foreground,
        focus: null_mut(),
        caret: null_mut(),
        foreground,
        thread_id,
    };

    if thread_id == 0 {
        return result;
    }

    let mut info: GUITHREADINFO = zeroed();
    info.cbSize = size_of::<GUITHREADINFO>() as u32;
    if GetGUIThreadInfo(thread_id, &mut info) != FALSE {
        result.active = if info.hwndActive.is_null() {
            foreground
        } else {
            info.hwndActive
        };
        result.focus = info.hwndFocus;
        result.caret = info.hwndCaret;
    }
    result
}

pub unsafe fn window_at_cursor() -> HWND {
    let mut screen = POINT::default();
    if GetCursorPos(&mut screen) == FALSE {
        return null_mut();
    }

    let mut current = WindowFromPoint(screen);
    if current.is_null() {
        return null_mut();
    }

    // WindowFromPoint is often a top-level or intermediate child window. Descend to the
    // deepest visible, enabled child so browser/Office editor surfaces have a chance to
    // contribute their own IME window.
    for _ in 0..12 {
        let mut client = screen;
        if ScreenToClient(current, &mut client) == FALSE {
            break;
        }
        let child = ChildWindowFromPointEx(
            current,
            client,
            CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
        );
        if child.is_null() || child == current {
            break;
        }
        current = child;
    }

    current
}

pub unsafe fn root_window(hwnd: HWND) -> HWND {
    if hwnd.is_null() {
        return null_mut();
    }
    let root = GetAncestor(hwnd, GA_ROOT);
    if root.is_null() {
        hwnd
    } else {
        root
    }
}

unsafe fn add_window_chain(list: &mut Vec<HWND>, seen: &mut HashSet<usize>, hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }

    let original = hwnd;
    let mut current = hwnd;
    for _ in 0..8 {
        add_window_candidate(list, seen, current);
        let parent = GetParent(current);
        if parent.is_null() || parent == current {
            break;
        }
        current = parent;
    }

    add_window_candidate(list, seen, GetAncestor(original, GA_ROOT));
    add_window_candidate(list, seen, GetAncestor(original, GA_ROOTOWNER));
}

unsafe fn add_window_candidate(list: &mut Vec<HWND>, seen: &mut HashSet<usize>, hwnd: HWND) {
    if hwnd.is_null() || IsWindow(hwnd) == FALSE {
        return;
    }
    if seen.insert(hwnd as usize) {
        list.push(hwnd);
    }
}

unsafe fn window_thread(hwnd: HWND) -> u32 {
    if hwnd.is_null() {
        0
    } else {
        GetWindowThreadProcessId(hwnd, null_mut())
    }
}

unsafe fn thread_language(thread_id: u32) -> u16 {
    if thread_id == 0 {
        return 0;
    }
    let layout = GetKeyboardLayout(thread_id);
    if layout.is_null() {
        0
    } else {
        (layout as usize & 0xffff) as u16
    }
}

fn cache_key(thread_id: u32, hwnd: HWND) -> CacheKey {
    if thread_id != 0 {
        CacheKey::Thread(thread_id)
    } else {
        CacheKey::Window(hwnd as usize)
    }
}

fn first_non_null(values: &[HWND]) -> HWND {
    values
        .iter()
        .copied()
        .find(|hwnd| !hwnd.is_null())
        .unwrap_or(null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_masks_use_primary_language_bits() {
        assert_eq!(0x0412u16 & 0x03ff, LANG_KOREAN);
        assert_eq!(0x0411u16 & 0x03ff, LANG_JAPANESE);
    }
}
