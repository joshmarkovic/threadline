//! Read three owned sidebar controls from one Windows compositor frame.
//! Only badge-color and digit measurements leave the process.

#[cfg(any(windows, test))]
use serde_json::{Value, json};

#[cfg(any(windows, test))]
fn measure(bgra: &[[u8; 4]], width: usize, left: i32, top: i32) -> Value {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, 0, 0);
    let mut blue = 0;
    for (i, &[b, g, r, _]) in bgra.iter().enumerate() {
        if r.abs_diff(23) > 8 || g.abs_diff(62) > 8 || b.abs_diff(112) > 8 {
            continue;
        }
        let (x, y) = (left + (i % width) as i32, top + (i / width) as i32);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        blue += 1;
    }
    let (cx, cy) = if blue == 0 {
        x0 = 0;
        y0 = 0;
        (0.0, 0.0)
    } else {
        (f64::from(x0 + x1) / 2.0, f64::from(y0 + y1) / 2.0)
    };
    let radius = f64::from((x1 - x0).min(y1 - y0)) * 0.4;
    let (mut digit_x0, mut digit_y0, mut digit_x1, mut digit_y1) = (x1, y1, x0, y0);
    let mut white = 0;
    for (i, &[b, g, r, _]) in bgra.iter().enumerate() {
        if r.min(g).min(b) < 225 {
            continue;
        }
        let (x, y) = (left + (i % width) as i32, top + (i / width) as i32);
        let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
        if dx * dx + dy * dy < radius * radius {
            digit_x0 = digit_x0.min(x);
            digit_y0 = digit_y0.min(y);
            digit_x1 = digit_x1.max(x + 1);
            digit_y1 = digit_y1.max(y + 1);
            white += 1;
        }
    }
    json!({
        "BluePixels":blue, "WhitePixels":white, "Left":x0, "Top":y0, "Right":x1, "Bottom":y1,
        "DigitCenterX":if white == 0 { 0.0 } else { f64::from(digit_x0+digit_x1)/2.0-cx },
        "DigitCenterY":if white == 0 { 0.0 } else { f64::from(digit_y0+digit_y1)/2.0-cy }
    })
}

#[cfg(windows)]
mod capture {
    use super::{Value, json, measure};
    use std::{error::Error, sync::mpsc, time::Duration};
    use windows::{
        Foundation::TypedEventHandler,
        Graphics::{
            Capture::*,
            DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
            SizeInt32,
        },
        Win32::{
            Foundation::{HMODULE, HWND, POINT, RECT},
            Graphics::{
                Direct3D::*,
                Direct3D11::*,
                Dwm::*,
                Dxgi::{Common::*, IDXGIDevice},
                Gdi::ClientToScreen,
            },
            System::WinRT::{Direct3D11::*, Graphics::Capture::IGraphicsCaptureItemInterop, *},
            UI::{HiDpi::*, WindowsAndMessaging::*},
        },
        core::{Interface, factory},
    };

    type Result<T> = std::result::Result<T, Box<dyn Error>>;

    fn geometry(hwnd: HWND, pid: u32, dpi: u32, size: SizeInt32) -> Result<(POINT, RECT)> {
        let mut actual_pid = 0;
        let mut client = RECT::default();
        let mut origin = POINT::default();
        let mut window = RECT::default();
        let mut extended = RECT::default();
        unsafe {
            if GetWindowThreadProcessId(hwnd, Some(&mut actual_pid)) == 0
                || actual_pid != pid
                || GetDpiForWindow(hwnd) != dpi
            {
                return Err("Fixture process or DPI changed.".into());
            }
            GetClientRect(hwnd, &mut client)?;
            ClientToScreen(hwnd, &mut origin).ok()?;
            GetWindowRect(hwnd, &mut window)?;
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut extended).cast(),
                size_of::<RECT>() as u32,
            )?;
        }
        let client_screen = RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + client.right,
            bottom: origin.y + client.bottom,
        };
        let mut offsets = Vec::new();
        for bounds in [extended, window, client_screen] {
            if bounds.right - bounds.left == size.Width && bounds.bottom - bounds.top == size.Height
            {
                offsets.push((origin.x - bounds.left, origin.y - bounds.top));
            }
        }
        offsets.sort_unstable();
        offsets.dedup();
        let [(x, y)] = offsets[..] else {
            return Err("No unique physical client origin for this frame.".into());
        };
        if x < 0 || y < 0 || x + client.right > size.Width || y + client.bottom > size.Height {
            return Err("Client outside captured frame.".into());
        }
        Ok((POINT { x, y }, client))
    }

    fn devices() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
        let mut error = None;
        for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
            let (mut device, mut context) = (None, None);
            let result = unsafe {
                D3D11CreateDevice(
                    None,
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
            };
            if let (Ok(()), Some(device), Some(context)) = (&result, device, context) {
                let dxgi: IDXGIDevice = device.cast()?;
                let winrt = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
                return Ok((device, context, winrt));
            }
            error = result.err();
        }
        Err(error.ok_or("No D3D capture device.")?.into())
    }

    fn read_row(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        row: &[i32; 4],
        offset: POINT,
        client: RECT,
    ) -> Result<Value> {
        let [left, top, right, bottom] = *row;
        if left < 0
            || top < 0
            || right <= left
            || bottom <= top
            || right > client.right
            || bottom > client.bottom
            || right - left > 2048
            || bottom - top > 256
        {
            return Err("Sidebar row outside owned client bounds.".into());
        }
        let left = left + (right - left) / 2;
        let (width, height) = ((right - left) as u32, (bottom - top) as u32);
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let source_box = D3D11_BOX {
            left: (left + offset.x) as u32,
            top: (top + offset.y) as u32,
            front: 0,
            right: (right + offset.x) as u32,
            bottom: (bottom + offset.y) as u32,
            back: 1,
        };
        let mut target = None;
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut target))?;
        }
        let target = target.ok_or("No staging texture.")?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let mut colors = Vec::with_capacity((width * height) as usize);
        unsafe {
            context.CopySubresourceRegion(&target, 0, 0, 0, 0, source, 0, Some(&source_box));
            context.Map(&target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            if mapped.pData.is_null() || mapped.RowPitch < width * 4 {
                context.Unmap(&target, 0);
                return Err("Invalid staging row pitch.".into());
            }
            for y in 0..height as usize {
                let origin = mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize);
                colors.extend_from_slice(std::slice::from_raw_parts(
                    origin.cast::<[u8; 4]>(),
                    width as usize,
                ));
            }
            context.Unmap(&target, 0);
        }
        Ok(measure(&colors, width as usize, left, top))
    }

    pub fn run() -> Result<()> {
        let values: Vec<i64> = std::env::args()
            .skip(1)
            .map(|a| a.parse())
            .collect::<std::result::Result<_, _>>()?;
        if values.len() != 15 {
            return Err("Expected the fixture HWND, PID, DPI and three row rectangles.".into());
        }
        let hwnd = HWND(usize::try_from(values[0])? as *mut _);
        let pid = u32::try_from(values[1])?;
        let dpi = u32::try_from(values[2])?;
        if hwnd.0.is_null() || pid == 0 || !(48..=768).contains(&dpi) {
            return Err("Invalid fixture identity.".into());
        }
        let rows: Vec<i32> = values[3..]
            .iter()
            .copied()
            .map(i32::try_from)
            .collect::<std::result::Result<_, _>>()?;
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
            if SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                .0
                .is_null()
            {
                return Err("Could not select physical DPI coordinates.".into());
            }
        }
        // Validate ownership before creating the window-scoped capture item.
        let mut owner = 0;
        if unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) } == 0 || owner != pid {
            return Err("Fixture window is not owned by the expected process.".into());
        }
        if !GraphicsCaptureSession::IsSupported()? {
            return Err("Window capture is unavailable.".into());
        }
        let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, _>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let size = item.Size()?;
        if size.Width <= 0
            || size.Height <= 0
            || i64::from(size.Width) * i64::from(size.Height) > 40_000_000
        {
            return Err("Invalid window size.".into());
        }
        let bounds = geometry(hwnd, pid, dpi, size)?;
        let (device, context, winrt) = devices()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsCursorCaptureEnabled(false)?;
        let (ready, receiver) = mpsc::sync_channel(1);
        let token = pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            let _ = ready.try_send(());
            Ok(())
        }))?;
        session.StartCapture()?;
        receiver.recv_timeout(Duration::from_secs(5))?;
        let frame = pool.TryGetNextFrame()?;
        if frame.ContentSize()? != size {
            return Err("The owned window changed size.".into());
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            texture.GetDesc(&mut desc);
        }
        if desc.Width != size.Width as u32
            || desc.Height != size.Height as u32
            || desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        {
            return Err("Unexpected capture surface dimensions or format.".into());
        }
        let samples = rows
            .as_chunks::<4>()
            .0
            .iter()
            .map(|row| read_row(&device, &context, &texture, row, bounds.0, bounds.1))
            .collect::<Result<Vec<_>>>()?;
        pool.RemoveFrameArrived(token)?;
        frame.Close()?;
        session.Close()?;
        pool.Close()?;
        if geometry(hwnd, pid, dpi, size)? != bounds {
            return Err("Fixture geometry changed while measuring.".into());
        }
        println!(
            "{}",
            json!({"dpi":dpi, "rows":samples, "client_offset":[bounds.0.x,bounds.0.y]})
        );
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match capture::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Owned-window compositor measurement unavailable: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This fixture requires Windows.");
    std::process::exit(78);
}

#[cfg(test)]
mod tests {
    #[test]
    fn badge_bounds_and_digit_center_use_client_coordinates() {
        let mut pixels = vec![[112, 62, 23, 255]; 25];
        pixels[12] = [255; 4];
        let measured = super::measure(&pixels, 5, 30, 20);
        assert_eq!(measured["BluePixels"], 24);
        assert_eq!(measured["WhitePixels"], 1);
        assert_eq!(measured["Right"], 35);
        assert_eq!(measured["Bottom"], 25);
        assert_eq!(measured["DigitCenterX"], 0.0);
        assert_eq!(measured["DigitCenterY"], 0.0);
        assert_eq!(super::measure(&[[255; 4]; 9], 3, 0, 0)["BluePixels"], 0);
    }
}
