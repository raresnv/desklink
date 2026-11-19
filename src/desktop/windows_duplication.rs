//! DXGI readback lives entirely on the capture worker. Unsupported/rotated outputs
//! use the scaled GDI backend instead. Pixel rows never include uninitialized padding.
use super::Display;
use anyhow::{bail, ensure, Context, Result};
use windows::{
    core::Interface,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
    },
};

pub(super) struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
pub(super) struct Duplication {
    output: IDXGIOutputDuplication,
    monitor: IDXGIOutput,
    approved: DXGI_OUTPUT_DESC,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
}
struct Acquired<'a>(&'a IDXGIOutputDuplication);
impl Drop for Acquired<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.ReleaseFrame();
        }
    }
}
struct Mapped<'a>(&'a ID3D11DeviceContext, &'a ID3D11Texture2D);
impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        unsafe {
            self.0.Unmap(self.1, 0);
        }
    }
}
impl Duplication {
    pub fn open(display: &Display) -> Result<Self> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            for index in 0..32 {
                let adapter = match factory.EnumAdapters1(index) {
                    Ok(a) => a,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e.into()),
                };
                for index in 0..32 {
                    let output = match adapter.EnumOutputs(index) {
                        Ok(o) => o,
                        Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                        Err(e) => return Err(e.into()),
                    };
                    let desc = output.GetDesc()?;
                    let r = desc.DesktopCoordinates;
                    if desc.Monitor.0 as usize as u32 != display.id
                        || !display.info.name.encode_utf16().eq(desc
                            .DeviceName
                            .iter()
                            .copied()
                            .take_while(|ch| *ch != 0))
                        || !desc.AttachedToDesktop.as_bool()
                        || r.left != display.info.x
                        || r.top != display.info.y
                        || (r.right - r.left) as u32 != display.info.width
                        || (r.bottom - r.top) as u32 != display.info.height
                    {
                        continue;
                    }
                    ensure!(
                        desc.Rotation == DXGI_MODE_ROTATION_IDENTITY,
                        "Rotated output uses GDI capture"
                    );
                    let (mut device, mut context) = (None, None);
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )?;
                    let device = device.context("No D3D11 device")?;
                    let context = context.context("No D3D11 context")?;
                    let monitor = output;
                    let approved = desc;
                    let output = monitor.cast::<IDXGIOutput1>()?.DuplicateOutput(&device)?;
                    let desc = output.GetDesc();
                    let (width, height) = (desc.ModeDesc.Width, desc.ModeDesc.Height);
                    ensure!(
                        width > 0
                            && height > 0
                            && u64::from(width) * u64::from(height) <= 16_777_216
                            && desc.ModeDesc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
                        "Unsupported duplication surface"
                    );
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
                    let mut staging = None;
                    device.CreateTexture2D(&desc, None, Some(&mut staging))?;
                    return Ok(Self {
                        output,
                        monitor,
                        approved,
                        context,
                        staging: staging.context("No readback texture")?,
                        width,
                        height,
                    });
                }
            }
            bail!("No matching DXGI output")
        }
    }
    pub fn capture(&self) -> Result<Option<RawFrame>> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            match self.output.AcquireNextFrame(100, &mut info, &mut resource) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    self.verify_monitor()?;
                    return Ok(None);
                }
                result => result?,
            }
            let _acquired = Acquired(&self.output);
            self.verify_monitor()?;
            let source = resource
                .context("No duplicated frame")?
                .cast::<ID3D11Texture2D>()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            ensure!(
                desc.Width == self.width
                    && desc.Height == self.height
                    && desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
                "Display configuration changed"
            );
            self.context.CopyResource(&self.staging, &source);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let _mapped = Mapped(&self.context, &self.staging);
            let stride = self.width as usize * 4;
            ensure!(
                !mapped.pData.is_null() && mapped.RowPitch as usize >= stride,
                "Invalid readback layout"
            );
            let mut pixels = vec![0; stride * self.height as usize];
            for row in 0..self.height as usize {
                let source = std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    stride,
                );
                pixels[row * stride..(row + 1) * stride].copy_from_slice(source);
            }
            Ok(Some(RawFrame {
                width: self.width,
                height: self.height,
                pixels,
            }))
        }
    }
    fn verify_monitor(&self) -> Result<()> {
        let current = unsafe { self.monitor.GetDesc()? };
        ensure!(
            same_output(&self.approved, &current),
            "Display configuration changed; select the display and approve again"
        );
        Ok(())
    }
}

fn same_output(a: &DXGI_OUTPUT_DESC, b: &DXGI_OUTPUT_DESC) -> bool {
    b.AttachedToDesktop.as_bool()
        && a.Monitor == b.Monitor
        && a.DeviceName == b.DeviceName
        && a.Rotation == b.Rotation
        && a.DesktopCoordinates == b.DesktopCoordinates
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn topology_changes_invalidate_the_approved_output() {
        let original = DXGI_OUTPUT_DESC {
            AttachedToDesktop: true.into(),
            DesktopCoordinates: windows::Win32::Foundation::RECT {
                right: 3600,
                bottom: 2068,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(same_output(&original, &original));
        let mut moved = original;
        moved.DesktopCoordinates.left = 10;
        assert!(!same_output(&original, &moved));
        let mut detached = original;
        detached.AttachedToDesktop = false.into();
        assert!(!same_output(&original, &detached));
        let mut replaced = original;
        replaced.DeviceName[0] = 65;
        assert!(!same_output(&original, &replaced));
        let mut rotated = original;
        rotated.Rotation = DXGI_MODE_ROTATION_ROTATE90;
        assert!(!same_output(&original, &rotated));
    }
}
