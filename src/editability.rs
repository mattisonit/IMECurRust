use crate::ime::window_at_cursor;
use crate::win::*;
use std::ffi::c_void;
use std::mem::{size_of, transmute, zeroed};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

const PROBE_CACHE_DURATION: Duration = Duration::from_millis(175);
const MAX_UIA_PARENT_DEPTH: usize = 6;
const CLASS_NAME_CAPACITY: usize = 128;
const CONTROL_MESSAGE_TIMEOUT_MS: u32 = 25;

const UIA_CONTROL_TYPE_PROPERTY_ID: i32 = 30003;
const UIA_IS_KEYBOARD_FOCUSABLE_PROPERTY_ID: i32 = 30009;
const UIA_IS_ENABLED_PROPERTY_ID: i32 = 30010;
const UIA_IS_VALUE_PATTERN_AVAILABLE_PROPERTY_ID: i32 = 30043;
const UIA_VALUE_IS_READ_ONLY_PROPERTY_ID: i32 = 30046;
const UIA_LEGACY_IACCESSIBLE_STATE_PROPERTY_ID: i32 = 30096;
const UIA_IS_TEXT_EDIT_PATTERN_AVAILABLE_PROPERTY_ID: i32 = 30149;

const UIA_EDIT_CONTROL_TYPE_ID: i32 = 50004;
const UIA_TEXT_CONTROL_TYPE_ID: i32 = 50020;
const UIA_DOCUMENT_CONTROL_TYPE_ID: i32 = 50030;

const STATE_SYSTEM_UNAVAILABLE: i32 = 0x0000_0001;
const STATE_SYSTEM_READONLY: i32 = 0x0000_0040;

const CLSID_CUIAUTOMATION: GUID = GUID {
    Data1: 0xff48dba4,
    Data2: 0x60ef,
    Data3: 0x4201,
    Data4: [0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e],
};

const IID_IUIAUTOMATION: GUID = GUID {
    Data1: 0x30cbe57d,
    Data2: 0xd9d0,
    Data3: 0x452a,
    Data4: [0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Editability {
    Editable,
    ReadOnly,
    Unknown,
}

#[derive(Clone, Copy)]
struct CachedProbe {
    point: POINT,
    window: HWND,
    result: Editability,
    tick: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeEvidence {
    Editable,
    ReadOnly,
    SelectableText,
    Unknown,
}

/// Determines whether the I-Beam under the pointer represents an editable text
/// surface or selectable, read-only text.
pub struct EditabilityDetector {
    automation: *mut c_void,
    raw_view_walker: *mut c_void,
    co_initialized: bool,
    last_probe: Option<CachedProbe>,
}

impl EditabilityDetector {
    /// Creates the UI Automation client used as a fallback for non-standard
    /// controls such as browsers, Electron, WPF, and WinUI.
    pub fn new() -> Self {
        unsafe { Self::new_unsafe() }
    }

    unsafe fn new_unsafe() -> Self {
        let init_result = CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED);
        let co_initialized = init_result == S_OK || init_result == S_FALSE;

        let mut automation = null_mut();
        let create_result = CoCreateInstance(
            &CLSID_CUIAUTOMATION,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IUIAUTOMATION,
            &mut automation,
        );
        if create_result < 0 {
            automation = null_mut();
        }

        let raw_view_walker = if automation.is_null() {
            null_mut()
        } else {
            get_raw_view_walker(automation).unwrap_or(null_mut())
        };

        Self {
            automation,
            raw_view_walker,
            co_initialized,
            last_probe: None,
        }
    }

    /// Returns `ReadOnly` only when a standard control or UI Automation
    /// exposes positive evidence that text cannot be edited. Unknown custom
    /// frameworks retain the previous I-Beam-only behavior.
    pub fn at_cursor(&mut self) -> Editability {
        unsafe { self.at_cursor_unsafe() }
    }

    unsafe fn at_cursor_unsafe(&mut self) -> Editability {
        let mut point = POINT::default();
        if GetCursorPos(&mut point) == FALSE {
            return Editability::Unknown;
        }

        let window = window_at_cursor();
        if window.is_null() {
            return Editability::Unknown;
        }

        let now = Instant::now();
        if let Some(cached) = self.last_probe {
            if cached.window == window
                && cached.point.x == point.x
                && cached.point.y == point.y
                && now
                    .checked_duration_since(cached.tick)
                    .is_some_and(|age| age <= PROBE_CACHE_DURATION)
            {
                return cached.result;
            }
        }

        let result = classify_standard_window(window)
            .unwrap_or_else(|| self.classify_with_uia(point));
        self.last_probe = Some(CachedProbe {
            point,
            window,
            result,
            tick: now,
        });
        result
    }

    unsafe fn classify_with_uia(&self, point: POINT) -> Editability {
        if self.automation.is_null() {
            return Editability::Unknown;
        }

        let Some(mut element) = element_from_point(self.automation, point) else {
            return Editability::Unknown;
        };

        let mut saw_selectable_text = false;
        for _ in 0..MAX_UIA_PARENT_DEPTH {
            match inspect_element(element) {
                NodeEvidence::Editable => {
                    release_com(element);
                    return Editability::Editable;
                }
                NodeEvidence::ReadOnly => {
                    release_com(element);
                    return Editability::ReadOnly;
                }
                NodeEvidence::SelectableText => saw_selectable_text = true,
                NodeEvidence::Unknown => {}
            }

            if self.raw_view_walker.is_null() {
                break;
            }
            let Some(parent) = tree_walker_parent(self.raw_view_walker, element) else {
                break;
            };
            release_com(element);
            element = parent;
        }

        release_com(element);
        if saw_selectable_text {
            Editability::ReadOnly
        } else {
            Editability::Unknown
        }
    }
}

impl Drop for EditabilityDetector {
    fn drop(&mut self) {
        unsafe {
            if !self.raw_view_walker.is_null() {
                release_com(self.raw_view_walker);
                self.raw_view_walker = null_mut();
            }
            if !self.automation.is_null() {
                release_com(self.automation);
                self.automation = null_mut();
            }
            if self.co_initialized {
                CoUninitialize();
                self.co_initialized = false;
            }
        }
    }
}

unsafe fn classify_standard_window(window: HWND) -> Option<Editability> {
    if IsWindowEnabled(window) == FALSE {
        return Some(Editability::ReadOnly);
    }

    let class_name = window_class_name(window)?;
    let normalized = class_name.to_ascii_lowercase();

    if normalized == "edit" || normalized.starts_with("richedit") {
        let style = get_window_long_ptr(window, GWL_STYLE) as u32;
        return Some(if style & ES_READONLY != 0 {
            Editability::ReadOnly
        } else {
            Editability::Editable
        });
    }

    if normalized == "scintilla" {
        let mut read_only = 0usize;
        let sent = SendMessageTimeoutW(
            window,
            SCI_GETREADONLY,
            0,
            0,
            SMTO_BLOCK | SMTO_ABORTIFHUNG,
            CONTROL_MESSAGE_TIMEOUT_MS,
            &mut read_only,
        );
        if sent != 0 {
            return Some(if read_only != 0 {
                Editability::ReadOnly
            } else {
                Editability::Editable
            });
        }
    }

    if normalized == "static" {
        return Some(Editability::ReadOnly);
    }

    None
}

unsafe fn window_class_name(window: HWND) -> Option<String> {
    let mut buffer = [0u16; CLASS_NAME_CAPACITY];
    let length = GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32);
    if length <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

unsafe fn inspect_element(element: *mut c_void) -> NodeEvidence {
    if property_bool(element, UIA_IS_ENABLED_PROPERTY_ID) == Some(false) {
        return NodeEvidence::ReadOnly;
    }

    let value_pattern = property_bool(element, UIA_IS_VALUE_PATTERN_AVAILABLE_PROPERTY_ID)
        .unwrap_or(false);
    if value_pattern {
        return match property_bool(element, UIA_VALUE_IS_READ_ONLY_PROPERTY_ID) {
            Some(false) => NodeEvidence::Editable,
            Some(true) => NodeEvidence::ReadOnly,
            None => NodeEvidence::Unknown,
        };
    }

    if property_bool(element, UIA_IS_TEXT_EDIT_PATTERN_AVAILABLE_PROPERTY_ID) == Some(true) {
        return NodeEvidence::Editable;
    }

    let control_type = property_i32(element, UIA_CONTROL_TYPE_PROPERTY_ID);
    let legacy_state = property_i32(element, UIA_LEGACY_IACCESSIBLE_STATE_PROPERTY_ID)
        .unwrap_or(0);
    if legacy_state & STATE_SYSTEM_UNAVAILABLE != 0 {
        return NodeEvidence::ReadOnly;
    }

    if control_type == Some(UIA_EDIT_CONTROL_TYPE_ID) {
        if legacy_state & STATE_SYSTEM_READONLY != 0
            || property_bool(element, UIA_IS_KEYBOARD_FOCUSABLE_PROPERTY_ID) == Some(false)
        {
            return NodeEvidence::ReadOnly;
        }
        return NodeEvidence::Unknown;
    }

    if legacy_state & STATE_SYSTEM_READONLY != 0
        || control_type == Some(UIA_TEXT_CONTROL_TYPE_ID)
        || control_type == Some(UIA_DOCUMENT_CONTROL_TYPE_ID)
    {
        NodeEvidence::SelectableText
    } else {
        NodeEvidence::Unknown
    }
}

unsafe fn element_from_point(automation: *mut c_void, point: POINT) -> Option<*mut c_void> {
    type Method = unsafe extern "system" fn(*mut c_void, POINT, *mut *mut c_void) -> HRESULT;
    let method: Method = transmute(com_method_address(automation, 7)?);
    let mut element = null_mut();
    let result = method(automation, point, &mut element);
    if result >= 0 && !element.is_null() {
        Some(element)
    } else {
        None
    }
}

unsafe fn get_raw_view_walker(automation: *mut c_void) -> Option<*mut c_void> {
    type Method = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
    let method: Method = transmute(com_method_address(automation, 16)?);
    let mut walker = null_mut();
    let result = method(automation, &mut walker);
    if result >= 0 && !walker.is_null() {
        Some(walker)
    } else {
        None
    }
}

unsafe fn tree_walker_parent(walker: *mut c_void, element: *mut c_void) -> Option<*mut c_void> {
    type Method = unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *mut *mut c_void,
    ) -> HRESULT;
    let method: Method = transmute(com_method_address(walker, 3)?);
    let mut parent = null_mut();
    let result = method(walker, element, &mut parent);
    if result >= 0 && !parent.is_null() {
        Some(parent)
    } else {
        None
    }
}

unsafe fn property_bool(element: *mut c_void, property_id: i32) -> Option<bool> {
    let mut value: VARIANT = zeroed();
    if !get_property(element, property_id, &mut value) {
        VariantClear(&mut value);
        return None;
    }
    let result = if value.vt == VT_BOOL {
        Some(value.data.bool_val != VARIANT_FALSE)
    } else {
        None
    };
    VariantClear(&mut value);
    result
}

unsafe fn property_i32(element: *mut c_void, property_id: i32) -> Option<i32> {
    let mut value: VARIANT = zeroed();
    if !get_property(element, property_id, &mut value) {
        VariantClear(&mut value);
        return None;
    }
    let result = if value.vt == VT_I4 {
        Some(value.data.l_val)
    } else {
        None
    };
    VariantClear(&mut value);
    result
}

unsafe fn get_property(element: *mut c_void, property_id: i32, value: *mut VARIANT) -> bool {
    type Method =
        unsafe extern "system" fn(*mut c_void, i32, BOOL, *mut VARIANT) -> HRESULT;
    let Some(address) = com_method_address(element, 11) else {
        return false;
    };
    let method: Method = transmute(address);
    // Ignore UI Automation's default values so an unsupported IsEnabled or
    // IsReadOnly property is not mistaken for real read-only evidence.
    method(element, property_id, TRUE, value) >= 0
}

unsafe fn release_com(object: *mut c_void) {
    if object.is_null() {
        return;
    }
    type Method = unsafe extern "system" fn(*mut c_void) -> u32;
    if let Some(address) = com_method_address(object, 2) {
        let method: Method = transmute(address);
        method(object);
    }
}

unsafe fn com_method_address(object: *mut c_void, index: usize) -> Option<usize> {
    if object.is_null() {
        return None;
    }
    let vtable = *(object as *mut *mut usize);
    if vtable.is_null() {
        return None;
    }
    let address = *vtable.add(index);
    (address != 0).then_some(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_and_unavailable_states_are_distinct_bits() {
        assert_eq!(STATE_SYSTEM_READONLY, 0x40);
        assert_eq!(STATE_SYSTEM_UNAVAILABLE, 0x01);
    }

    #[test]
    fn variant_layout_matches_automation_abi() {
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<VARIANT>(), 24);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(size_of::<VARIANT>(), 16);
    }
}
