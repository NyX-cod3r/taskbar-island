//! One GPU stack for the whole process.
//!
//! Both the pill and the flyout need D3D11, DXGI, DirectComposition, Direct2D
//! and DirectWrite. Creating two independent devices duplicated driver-side
//! allocations and cost ~27 MB of working set; this module creates the stack
//! once and hands out shared handles. Each window still owns its own swapchain,
//! composition target/visual and D2D device context; only the device-level
//! objects are shared.

use std::cell::RefCell;
use std::rc::Rc;

use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct2D::{D2D1CreateDevice, ID2D1Device};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIFactory2};

pub struct Gpu {
    pub d3d: ID3D11Device,
    pub dxgi_factory: IDXGIFactory2,
    pub comp: IDCompositionDevice,
    pub d2d: ID2D1Device,
    pub dwrite: IDWriteFactory,
}

thread_local! {
    static GPU: RefCell<Option<Rc<Gpu>>> = const { RefCell::new(None) };
}

impl Gpu {
    fn create() -> Result<Self> {
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut feature_level = core::mem::MaybeUninit::uninit();
        // Small, single-purpose block: create the D3D11 device.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                Some(feature_level.as_mut_ptr()),
                None,
            )?;
        }
        let d3d = d3d_device.expect("d3d device");
        let dxgi_device: IDXGIDevice = d3d.cast()?;
        // Small, single-purpose block: derive the rest of the stack from it.
        unsafe {
            let adapter = dxgi_device.GetAdapter()?;
            let dxgi_factory: IDXGIFactory2 = adapter.GetParent()?;
            let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let d2d: ID2D1Device = D2D1CreateDevice(&dxgi_device, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            Ok(Self {
                d3d,
                dxgi_factory,
                comp,
                d2d,
                dwrite,
            })
        }
    }
}

/// Returns the shared GPU stack, creating it on first use.
pub fn shared() -> Result<Rc<Gpu>> {
    if let Some(gpu) = GPU.with(|cell| cell.borrow().clone()) {
        return Ok(gpu);
    }
    let gpu = Rc::new(Gpu::create()?);
    GPU.with(|cell| *cell.borrow_mut() = Some(gpu.clone()));
    Ok(gpu)
}
