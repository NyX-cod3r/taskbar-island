//! render.rs — GPU rendering for the pill.
//!
//! Pipeline: D3D11 device → DXGI swapchain (flip, `DXGI_ALPHA_MODE_PREMULTIPLIED`)
//! hosted in a DirectComposition visual tree on the HWND → Direct2D device
//! context draws into the backbuffer. This is what gives the window per-pixel
//! alpha without `WS_EX_LAYERED`, so the pill blends with the real taskbar
//! pixels underneath (no blur, just alpha — matching the taskbar highlight).
//!
//! Renders on demand only: once at startup, on resize, and (later phases)
//! while an animation is active. There is no frame loop.

use core::mem::ManuallyDrop;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT, D2D1CreateDevice, ID2D1Bitmap1,
    ID2D1Device, ID2D1DeviceContext, ID2D1Image, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectComposition::DCompositionCreateDevice;
use windows::Win32::Graphics::DirectComposition::IDCompositionDevice;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED as DXGI_ALPHA_PREMULTIPLIED, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::core::{Interface, Result};

/// Taskbar-highlight colors (dark theme). Phase 2's theme.rs will make these dynamic.
const FILL: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 };
const BORDER: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.10 };
const CORNER_RADIUS_DIP: f32 = 8.0;
const BORDER_WIDTH_DIP: f32 = 1.0;

pub struct Renderer {
    swapchain: IDXGISwapChain1,
    comp_device: IDCompositionDevice,
    d2d_context: ID2D1DeviceContext,
    /// `None` only transiently while resizing the backbuffer.
    target_bitmap: Option<ID2D1Bitmap1>,
    fill_brush: ID2D1SolidColorBrush,
    border_brush: ID2D1SolidColorBrush,
    /// Backbuffer size in physical pixels.
    size: (u32, u32),
    /// DIPs → physical pixels (dpi / 96).
    scale: f32,
}

impl Renderer {
    /// Build the whole device chain for `hwnd` at `size` physical px.
    ///
    /// # Safety
    /// `hwnd` must be a valid window created with `WS_EX_NOREDIRECTIONBITMAP`
    /// (required by `IDCompositionDevice::CreateTargetForHwnd`).
    pub unsafe fn create(hwnd: HWND, size: (u32, u32), scale: f32) -> Result<Self> {
        // --- Direct3D device (BGRA support is required by Direct2D-on-DXGI) ---
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut feature_level = core::mem::MaybeUninit::uninit();
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
        let d3d_device = d3d_device.expect("D3D11CreateDevice succeeded without a device");

        // --- DXGI pieces: the factory that created the device's adapter, plus
        //     the device itself for swapchain/DComp interop. ---
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let adapter = dxgi_device.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;

        // --- Premultiplied-alpha flip swapchain hosted by the window. ---
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_PREMULTIPLIED,
            ..Default::default()
        };
        let swapchain = factory.CreateSwapChainForHwnd(&d3d_device, hwnd, &desc, None, None)?;
        // We draw on demand; never let DXGI hijack F11/Alt+Enter.
        let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);

        // --- DirectComposition tree: target(root(visual(content = swapchain))) ---
        let comp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
        let comp_target = comp_device.CreateTargetForHwnd(hwnd, true)?;
        let comp_visual = comp_device.CreateVisual()?;
        comp_visual.SetContent(&swapchain)?;
        comp_target.SetRoot(&comp_visual)?;
        comp_device.Commit()?;

        // --- Direct2D device + context; backbuffer wrapped as a D2D bitmap. ---
        let d2d_device: ID2D1Device = D2D1CreateDevice(&dxgi_device, None)?;
        let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let (target_bitmap, fill_brush, border_brush) =
            Self::make_target(&d2d_context, &swapchain, scale)?;

        Ok(Self { swapchain, comp_device, d2d_context, target_bitmap: Some(target_bitmap), fill_brush, border_brush, size, scale })
    }

    /// Backbuffer as a D2D target bitmap + the two theme brushes.
    /// The bitmap's DPI is the monitor DPI, so we can draw in logical DIPs.
    unsafe fn make_target(
        ctx: &ID2D1DeviceContext,
        swapchain: &IDXGISwapChain1,
        scale: f32,
    ) -> Result<(ID2D1Bitmap1, ID2D1SolidColorBrush, ID2D1SolidColorBrush)> {
        let back_buffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;
        let surface: IDXGISurface = back_buffer.cast()?;
        let dpi = 96.0 * scale;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_R8G8B8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        ctx.SetTarget(&bitmap);
        let fill = ctx.CreateSolidColorBrush(&FILL, None)?;
        let border = ctx.CreateSolidColorBrush(&BORDER, None)?;
        Ok((bitmap, fill, border))
    }

    /// Current backbuffer size in physical pixels (used by later phases).
    #[allow(dead_code)]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Resize the backbuffer for a new physical-pixel size. No-op if unchanged.
    pub unsafe fn resize(&mut self, size: (u32, u32), scale: f32) -> Result<()> {
        if size == self.size && scale == self.scale {
            return Ok(());
        }
        // Detach and drop the bitmap before resizing the buffer it wraps.
        self.d2d_context.SetTarget(None::<&ID2D1Image>);
        self.target_bitmap = None;
        self.swapchain.ResizeBuffers(0, size.0, size.1, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        let (bitmap, fill, border) = Self::make_target(&self.d2d_context, &self.swapchain, scale)?;
        self.target_bitmap = Some(bitmap);
        self.fill_brush = fill;
        self.border_brush = border;
        self.size = size;
        self.scale = scale;
        Ok(())
    }

    /// Draw the pill once and present. Static content → single Present, then
    /// let DirectComposition pick up the new frame.
    pub unsafe fn draw(&mut self) -> Result<()> {
        let w = self.size.0 as f32 / self.scale;
        let h = self.size.1 as f32 / self.scale;
        let radius = CORNER_RADIUS_DIP.min(w.min(h) / 2.0);
        // Inset the stroked rect by half the border width so the full 1 DIP
        // border stays inside the pill instead of being clipped at the edge.
        let inset = BORDER_WIDTH_DIP / 2.0;
        let shape = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h },
            radiusX: radius,
            radiusY: radius,
        };
        let stroke = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: inset, top: inset, right: w - inset, bottom: h - inset },
            radiusX: (radius - inset).max(0.0),
            radiusY: (radius - inset).max(0.0),
        };

        self.d2d_context.BeginDraw();
        self.d2d_context.Clear(None); // fully transparent — taskbar shows through
        self.d2d_context.FillRoundedRectangle(&shape, &self.fill_brush);
        self.d2d_context.DrawRoundedRectangle(
            &stroke,
            &self.border_brush,
            BORDER_WIDTH_DIP,
            None::<&ID2D1StrokeStyle>,
        );
        self.d2d_context.EndDraw(None, None)?;

        self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        // Commit after present so DirectComposition composites the new frame.
        self.comp_device.Commit()?;
        Ok(())
    }
}

// The ID2D1Device that produced d2d_context is kept alive by the context
// itself (COM refcount), so dropping it here is fine.
