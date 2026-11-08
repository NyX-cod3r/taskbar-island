//! uia.rs — UI Automation names for the icon-only controls.
//!
//! The island is one window, so the default HWND provider cannot describe the
//! rail buttons or the shelf toolbar. `WM_GETOBJECT` hands UIA a fragment
//! provider over a snapshot of the visible controls; every node's `Name` is
//! the same string its tooltip shows.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;

use windows::core::{implement, Error, Result, BSTR};
use windows::Win32::Foundation::{E_NOTIMPL, HWND, RECT, VARIANT_BOOL};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{
    VARIANT, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_I4,
};
use windows::Win32::UI::Accessibility::{
    IRawElementProviderFragment, IRawElementProviderFragmentRoot,
    IRawElementProviderFragmentRoot_Impl, IRawElementProviderFragment_Impl,
    IRawElementProviderSimple, IRawElementProviderSimple_Impl, NavigateDirection,
    NavigateDirection_FirstChild, NavigateDirection_LastChild, NavigateDirection_NextSibling,
    NavigateDirection_Parent, NavigateDirection_PreviousSibling, ProviderOptions,
    ProviderOptions_ServerSideProvider, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
    UIA_HasKeyboardFocusPropertyId, UIA_HelpTextPropertyId, UIA_IsControlElementPropertyId,
    UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_NamePropertyId,
    UIA_PaneControlTypeId, UiaAppendRuntimeId, UiaHostProviderFromHwnd, UiaRect,
    UIA_CONTROLTYPE_ID, UIA_E_ELEMENTNOTAVAILABLE, UIA_PATTERN_ID, UIA_PROPERTY_ID,
};

/// One exposed control, already resolved to physical screen pixels.
#[derive(Debug, Clone)]
pub struct A11yNode {
    /// Equals the tooltip title, with the optional detail appended.
    pub name: String,
    pub help: String,
    pub rect: RECT,
    pub enabled: bool,
    pub focus: bool,
}

#[derive(Debug)]
struct Shared {
    hwnd: HWND,
    window: RECT,
    nodes: Vec<A11yNode>,
}

#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
struct IslandProvider {
    shared: Rc<Shared>,
    /// `None` for the root fragment, otherwise the node index.
    index: Option<usize>,
}

impl IslandProvider {
    fn new(shared: Rc<Shared>, index: Option<usize>) -> Self {
        Self { shared, index }
    }

    fn child(&self, index: usize) -> Result<IRawElementProviderFragment> {
        let fragment: IRawElementProviderFragment =
            IslandProvider::new(self.shared.clone(), Some(index)).into();
        Ok(fragment)
    }

    fn root_fragment(&self) -> Result<IRawElementProviderFragment> {
        let fragment: IRawElementProviderFragment =
            IslandProvider::new(self.shared.clone(), None).into();
        Ok(fragment)
    }

    fn root_provider(&self) -> IRawElementProviderFragmentRoot {
        IslandProvider::new(self.shared.clone(), None).into()
    }

    fn node(&self) -> Option<&A11yNode> {
        self.index.and_then(|index| self.shared.nodes.get(index))
    }

    fn runtime_id(&self) -> Result<*mut SAFEARRAY> {
        // SAFETY: the vector is a fresh owned SAFEARRAY; both elements are
        // written before it is returned to UIA, which owns the result.
        unsafe {
            let array = SafeArrayCreateVector(VT_I4, 0, 2);
            if array.is_null() {
                return Err(Error::from_hresult(
                    windows::Win32::Foundation::E_OUTOFMEMORY,
                ));
            }
            let first: i32 = UiaAppendRuntimeId as i32;
            let second: i32 = match self.index {
                Some(index) => 1000 + index as i32,
                None => 1,
            };
            SafeArrayPutElement(array, &0, &first as *const i32 as *const c_void)?;
            SafeArrayPutElement(array, &1, &second as *const i32 as *const c_void)?;
            Ok(array)
        }
    }
}

fn name_variant(name: &str) -> VARIANT {
    let mut value = VARIANT::default();
    value.Anonymous.Anonymous = ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_BSTR,
        Anonymous: VARIANT_0_0_0 {
            bstrVal: ManuallyDrop::new(BSTR::from(name)),
        },
        ..Default::default()
    });
    value
}

fn int_variant(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    variant.Anonymous.Anonymous = ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_I4,
        Anonymous: VARIANT_0_0_0 { lVal: value },
        ..Default::default()
    });
    variant
}

fn bool_variant(value: bool) -> VARIANT {
    let mut variant = VARIANT::default();
    variant.Anonymous.Anonymous = ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_BOOL,
        Anonymous: VARIANT_0_0_0 {
            boolVal: VARIANT_BOOL(if value { -1 } else { 0 }),
        },
        ..Default::default()
    });
    variant
}

fn control_type_variant(kind: UIA_CONTROLTYPE_ID) -> VARIANT {
    int_variant(kind.0)
}

fn unavailable() -> Error {
    Error::from_hresult(windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

impl IRawElementProviderSimple_Impl for IslandProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _patternid: UIA_PATTERN_ID) -> Result<windows::core::IUnknown> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetPropertyValue(&self, propertyid: UIA_PROPERTY_ID) -> Result<VARIANT> {
        if propertyid == UIA_NamePropertyId {
            return Ok(name_variant(match self.node() {
                Some(node) => &node.name,
                None => "Taskbar island",
            }));
        }
        if propertyid == UIA_ControlTypePropertyId {
            return Ok(control_type_variant(match self.node() {
                Some(_) => UIA_ButtonControlTypeId,
                None => UIA_PaneControlTypeId,
            }));
        }
        if propertyid == UIA_IsEnabledPropertyId {
            return Ok(bool_variant(
                self.node().map(|node| node.enabled).unwrap_or(true),
            ));
        }
        if propertyid == UIA_HasKeyboardFocusPropertyId {
            return Ok(bool_variant(
                self.node().map(|node| node.focus).unwrap_or(false),
            ));
        }
        if propertyid == UIA_IsKeyboardFocusablePropertyId {
            return Ok(bool_variant(self.index.is_some()));
        }
        if propertyid == UIA_IsControlElementPropertyId {
            return Ok(bool_variant(true));
        }
        if propertyid == UIA_HelpTextPropertyId {
            if let Some(node) = self.node() {
                if !node.help.is_empty() {
                    return Ok(name_variant(&node.help));
                }
            }
        }
        Err(unavailable())
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        if self.index.is_some() {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        // SAFETY: the HWND belongs to this process and is alive while a
        // provider is handed to UIA.
        unsafe { UiaHostProviderFromHwnd(self.shared.hwnd) }
    }
}

impl IRawElementProviderFragment_Impl for IslandProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        match self.index {
            None => {
                if direction == NavigateDirection_FirstChild {
                    if self.shared.nodes.is_empty() {
                        return Err(unavailable());
                    }
                    return self.child(0);
                }
                if direction == NavigateDirection_LastChild {
                    return match self.shared.nodes.len() {
                        0 => Err(unavailable()),
                        len => self.child(len - 1),
                    };
                }
                Err(unavailable())
            }
            Some(index) => {
                if direction == NavigateDirection_Parent {
                    return self.root_fragment();
                }
                if direction == NavigateDirection_NextSibling {
                    return if index + 1 < self.shared.nodes.len() {
                        self.child(index + 1)
                    } else {
                        Err(unavailable())
                    };
                }
                if direction == NavigateDirection_PreviousSibling {
                    return if index > 0 {
                        self.child(index - 1)
                    } else {
                        Err(unavailable())
                    };
                }
                Err(unavailable())
            }
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        self.runtime_id()
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        let rect = match self.node() {
            Some(node) => node.rect,
            None => self.shared.window,
        };
        Ok(UiaRect {
            left: rect.left as f64,
            top: rect.top as f64,
            width: (rect.right - rect.left).max(0) as f64,
            height: (rect.bottom - rect.top).max(0) as f64,
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(self.root_provider())
    }
}

impl IRawElementProviderFragmentRoot_Impl for IslandProvider_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        if self.index.is_some() {
            return Err(unavailable());
        }
        let x = x as i32;
        let y = y as i32;
        for (index, node) in self.shared.nodes.iter().enumerate() {
            if x >= node.rect.left
                && x < node.rect.right
                && y >= node.rect.top
                && y < node.rect.bottom
            {
                return self.child(index);
            }
        }
        self.root_fragment()
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        match self.shared.nodes.iter().position(|node| node.focus) {
            Some(index) => self.child(index),
            None => Err(unavailable()),
        }
    }
}

/// Builds the root provider for one `WM_GETOBJECT` request.
pub fn provider(hwnd: HWND, window: RECT, nodes: Vec<A11yNode>) -> IRawElementProviderSimple {
    let shared = Rc::new(Shared {
        hwnd,
        window,
        nodes,
    });
    IslandProvider::new(shared, None).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_carry_their_payload() {
        let name = name_variant("Clear shelf");
        // SAFETY: the VT_BSTR branch was selected by the constructor.
        unsafe {
            assert_eq!(name.Anonymous.Anonymous.vt, VT_BSTR);
            assert_eq!(
                name.Anonymous.Anonymous.Anonymous.bstrVal.to_string(),
                "Clear shelf"
            );
        }
        let count = control_type_variant(UIA_ButtonControlTypeId);
        // SAFETY: VT_I4 branch.
        unsafe {
            assert_eq!(count.Anonymous.Anonymous.Anonymous.lVal, 50000);
        }
        let enabled = bool_variant(true);
        // SAFETY: VT_BOOL branch.
        unsafe {
            assert!(enabled.Anonymous.Anonymous.Anonymous.boolVal.0 != 0);
        }
    }
}
