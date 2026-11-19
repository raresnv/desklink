//! RandR scanout bounds and X11 pointer coordinates both use root pixels.
use super::{validate_x11_source, DesktopInfo, Display};
use anyhow::{ensure, Context, Result};
use image::{Rgba, RgbaImage};
use x11rb::{
    connection::Connection,
    image::{Image, PixelLayout},
    protocol::{
        randr::{self, ConnectionExt, GetCrtcInfoReply},
        xproto::{Screen, VisualClass, Visualtype},
    },
    rust_connection::RustConnection,
};

pub(super) struct X11 {
    conn: RustConnection,
    screen: usize,
}

impl X11 {
    pub fn connect() -> Result<Self> {
        let (conn, screen) =
            x11rb::connect(None).context("Could not connect to the X11 display")?;
        Ok(Self { conn, screen })
    }

    pub fn check_input_screen(&self, desktop: &DesktopInfo) -> Result<()> {
        // Enigo's libxdo backend sends absolute pointer motion to screen zero.
        ensure!(
            self.screen == 0,
            "Remote pointer control requires X11 screen zero"
        );
        let (right, bottom) = desktop.map_pointer(1.0, 1.0)?;
        ensure!(
            desktop.x >= 0
                && desktop.y >= 0
                && right <= i32::from(i16::MAX)
                && bottom <= i32::from(i16::MAX),
            "Selected display exceeds X11 pointer coordinate range"
        );
        Ok(())
    }

    fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen]
    }

    pub fn display(&self, id: u32) -> Result<Display> {
        Ok(self.output(id)?.0)
    }

    fn output(&self, id: u32) -> Result<(Display, randr::Crtc, u32)> {
        let resources = self
            .conn
            .randr_get_screen_resources_current(self.screen().root)?
            .reply()?;
        ensure!(
            resources.outputs.contains(&id),
            "The selected X11 output was disconnected"
        );
        let time = resources.config_timestamp;
        let output = self.conn.randr_get_output_info(id, time)?.reply()?;
        ensure!(
            output.status == randr::SetConfig::SUCCESS
                && output.connection != randr::Connection::DISCONNECTED
                && output.crtc != x11rb::NONE,
            "The selected X11 output is inactive"
        );
        let crtc = self.conn.randr_get_crtc_info(output.crtc, time)?.reply()?;
        Ok((
            output_display(id, &output.name, &crtc)?,
            output.crtc,
            crtc.timestamp,
        ))
    }

    fn verify_scanout(&self, approved: &Display) -> Result<u32> {
        let (current, crtc, generation) = self.output(approved.id)?;
        approved.verify_source(&current)?;
        let panning = self.conn.randr_get_panning(crtc)?;
        let transform = self.conn.randr_get_crtc_transform(crtc)?;
        let panning = panning.reply()?;
        require_identity_transform(transform.reply()?.current_transform)?;
        require_fixed_scanout(&panning)?;
        ensure!(
            generation == panning.timestamp,
            "X11 display changed while reading its geometry"
        );
        Ok(generation)
    }

    pub fn capture(&self, approved: &Display) -> Result<RgbaImage> {
        let generation = self.verify_scanout(approved)?;
        let (x, y, width, height) = capture_bounds(approved)?;
        let screen = self.screen();
        let (pixels, visual_id) = Image::get(&self.conn, screen.root, x, y, width, height)?;
        let current_generation = self.verify_scanout(approved)?;
        ensure!(
            generation == current_generation,
            "X11 display configuration changed during capture"
        );
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.visual_id == visual_id)
            .context("Unknown X11 capture visual")?;
        decode_image(&pixels, *visual)
    }
}

fn require_identity_transform(m: x11rb::protocol::render::Transform) -> Result<()> {
    // RandR uses 16.16 fixed point. A transformed bounding box can include
    // root pixels outside the polygon actually scanned out by this monitor.
    const ONE: i32 = 1 << 16;
    ensure!(
        [m.matrix11, m.matrix12, m.matrix13] == [ONE, 0, 0]
            && [m.matrix21, m.matrix22, m.matrix23] == [0, ONE, 0]
            && [m.matrix31, m.matrix32, m.matrix33] == [0, 0, ONE],
        "Disable the X11 output transform to share a fixed monitor"
    );
    Ok(())
}

fn require_fixed_scanout(panning: &randr::GetPanningReply) -> Result<()> {
    // With panning, GetCrtcInfo can report the whole virtual area.
    ensure!(
        panning.status == randr::SetConfig::SUCCESS && panning.width == 0 && panning.height == 0,
        "Disable X11 display panning to share a fixed monitor"
    );
    Ok(())
}

fn output_display(id: u32, name: &[u8], crtc: &GetCrtcInfoReply) -> Result<Display> {
    ensure!(
        crtc.status == randr::SetConfig::SUCCESS
            && crtc.mode != x11rb::NONE
            && crtc.outputs.contains(&id),
        "The selected X11 output changed"
    );
    let info = DesktopInfo {
        name: String::from_utf8_lossy(name).chars().take(80).collect(),
        x: i32::from(crtc.x),
        y: i32::from(crtc.y),
        width: u32::from(crtc.width),
        height: u32::from(crtc.height),
        scale: 1.0,
    };
    info.validate()?;
    Ok(Display { id, info })
}

fn capture_bounds(display: &Display) -> Result<(i16, i16, u16, u16)> {
    let d = &display.info;
    validate_x11_source(d.width, d.height)?;
    Ok((
        d.x.try_into()?,
        d.y.try_into()?,
        d.width.try_into()?,
        d.height.try_into()?,
    ))
}

fn decode_image(pixels: &Image<'_>, visual: Visualtype) -> Result<RgbaImage> {
    ensure!(
        visual.class == VisualClass::TRUE_COLOR,
        "X11 capture requires a TrueColor visual"
    );
    let layout = PixelLayout::from_visual_type(visual)?;
    ensure!(
        layout.depth() == pixels.depth(),
        "Unsupported X11 visual depth"
    );
    Ok(RgbaImage::from_fn(
        u32::from(pixels.width()),
        u32::from(pixels.height()),
        |x, y| {
            let (r, g, b) = layout.decode(pixels.get_pixel(x as u16, y as u16));
            Rgba([(r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8, 255])
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::image::{BitsPerPixel, ImageOrder, ScanlinePad};

    #[test]
    fn xft_dpi_never_changes_the_approved_root_pixels_or_pointer_endpoints() {
        // Each display is 1920 x 1080 in XCap's DPI-divided coordinates.
        // The adjacent output starts at the first pixel after the actual CRTC.
        for (dpi, x, y, w, h) in [
            (96, 1920, 200, 1920, 1080),
            (192, 3840, 400, 3840, 2160),
            (72, 1440, 150, 1440, 810),
        ] {
            let crtc = GetCrtcInfoReply {
                x,
                y,
                width: w,
                height: h,
                mode: 123,
                outputs: vec![7],
                ..Default::default()
            };
            let display = output_display(7, b"DP-1", &crtc).unwrap();
            assert_eq!(u32::from(w) * 96 / dpi, 1920);
            assert_eq!(capture_bounds(&display).unwrap(), (x, y, w, h), "{dpi} DPI");
            assert_eq!(
                display.info.map_pointer(0.0, 0.0).unwrap(),
                (i32::from(x), i32::from(y))
            );
            let right = i32::from(x) + i32::from(w);
            let bottom = i32::from(y) + i32::from(h);
            assert_eq!(
                display.info.map_pointer(1.0, 1.0).unwrap(),
                (right - 1, bottom - 1)
            );
        }
    }

    #[test]
    fn capture_rejects_scaled_or_translated_scanout_transforms() {
        let mut matrix = x11rb::protocol::render::Transform {
            matrix11: 65536,
            matrix22: 65536,
            matrix33: 65536,
            ..Default::default()
        };
        require_identity_transform(matrix).unwrap();
        matrix.matrix11 = 32768;
        assert!(require_identity_transform(matrix).is_err());
        matrix.matrix11 = 65536;
        matrix.matrix13 = 65536;
        assert!(require_identity_transform(matrix).is_err());
    }

    #[test]
    fn a_panning_area_can_never_be_used_as_the_fixed_monitor_rectangle() {
        let mut panning = randr::GetPanningReply::default();
        require_fixed_scanout(&panning).unwrap();
        panning.width = 7680;
        assert!(require_fixed_scanout(&panning).is_err());
        panning.width = 0;
        panning.height = 4320;
        assert!(require_fixed_scanout(&panning).is_err());
    }

    #[test]
    fn root_capture_rejects_missing_outputs_and_oversize_native_rectangles() {
        let crtc = GetCrtcInfoReply {
            mode: 123,
            outputs: vec![7],
            width: 7680,
            height: 4320,
            ..Default::default()
        };
        assert!(output_display(8, b"DP-2", &crtc).is_err());
        let display = output_display(7, b"DP-1", &crtc).unwrap();
        assert!(capture_bounds(&display).is_err());
    }

    #[test]
    fn x11_true_color_respects_row_padding_byte_order_and_visual_masks() {
        let visual = Visualtype {
            class: VisualClass::TRUE_COLOR,
            red_mask: 0xff0000,
            green_mask: 0xff00,
            blue_mask: 0xff,
            ..Default::default()
        };
        for (order, bytes) in [
            (ImageOrder::LsbFirst, [3, 2, 1, 255, 6, 5, 4, 128]),
            (ImageOrder::MsbFirst, [1, 2, 3, 255, 4, 5, 6, 128]),
        ] {
            let image = Image::new(
                1,
                2,
                ScanlinePad::Pad32,
                24,
                BitsPerPixel::B24,
                order,
                bytes.as_slice().into(),
            )
            .unwrap();
            assert_eq!(
                decode_image(&image, visual).unwrap().as_raw(),
                &[1, 2, 3, 255, 4, 5, 6, 255]
            );
            let swapped = Visualtype {
                red_mask: 0xff,
                blue_mask: 0xff0000,
                ..visual
            };
            assert_eq!(
                decode_image(&image, swapped).unwrap().get_pixel(0, 0).0,
                [3, 2, 1, 255]
            );
            assert!(decode_image(
                &image,
                Visualtype {
                    class: VisualClass::PSEUDO_COLOR,
                    ..visual
                }
            )
            .is_err());
        }
        assert!(Image::new(
            1,
            2,
            ScanlinePad::Pad32,
            24,
            BitsPerPixel::B24,
            ImageOrder::LsbFirst,
            [0u8; 7].as_slice().into()
        )
        .is_err());
    }
}
