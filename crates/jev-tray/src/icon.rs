//! Which icon file to show (jev-tray spec section 3): the state gives the
//! mark, the system gives the color, and the screen scale gives the size.

use crate::state::State;

/// The color of the icon's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    Black,
    White,
}

/// The sizes in `icons/`, in pixels, smallest first.
pub const SIZES: [u32; 5] = [16, 20, 24, 32, 48];

/// True where the bar recolors the icon itself: on macOS the black icon is
/// a template image.
pub const TEMPLATE: bool = cfg!(target_os = "macos");

/// The icon size in points on this system's bar.
pub const POINTS: u32 = if cfg!(target_os = "macos") {
    18
} else if cfg!(windows) {
    16
} else {
    24
};

/// The shade for a Windows taskbar, from the value `SystemUsesLightTheme`.
/// A light taskbar (1) gets black; a dark one (0) gets white. With no value,
/// the taskbar is dark, as on Windows 10 by default.
pub fn shade_for_windows(system_uses_light_theme: Option<u32>) -> Shade {
    match system_uses_light_theme {
        Some(0) | None => Shade::White,
        Some(_) => Shade::Black,
    }
}

/// The shade for this system's bar now: black on macOS, where it is a
/// template image.
#[cfg(target_os = "macos")]
pub fn shade_now() -> Shade {
    Shade::Black
}

/// The shade for this system's bar now. It reads the registry each time,
/// so a theme change shows at the next check.
#[cfg(windows)]
pub fn shade_now() -> Shade {
    shade_for_windows(windows_theme::system_uses_light_theme())
}

/// The shade for this system's bar now: white, because Linux panels do not
/// report their theme reliably, and most panels are dark.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn shade_now() -> Shade {
    Shade::White
}

/// The smallest committed size that covers `points` at `scale`, or the
/// largest one. A scale too large for every size, or a NaN scale, gives the
/// largest size; a zero or negative scale gives the smallest.
pub fn size_for(points: u32, scale: f64) -> u32 {
    let wanted = f64::from(points) * scale;
    // The 0.01 allows for float error in the scale, so that a product that
    // should equal a size exactly (16 x 1.25 = 20) still picks that size.
    SIZES
        .into_iter()
        .find(|&size| f64::from(size) >= wanted - 0.01)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// The files for one state and shade, in the order of `SIZES`.
macro_rules! files {
    ($name:literal) => {
        [
            include_bytes!(concat!("../icons/", $name, "-16.png")) as &[u8],
            include_bytes!(concat!("../icons/", $name, "-20.png")),
            include_bytes!(concat!("../icons/", $name, "-24.png")),
            include_bytes!(concat!("../icons/", $name, "-32.png")),
            include_bytes!(concat!("../icons/", $name, "-48.png")),
        ]
    };
}

const ON_BLACK: [&[u8]; 5] = files!("on-black");
const ON_WHITE: [&[u8]; 5] = files!("on-white");
const OFF_BLACK: [&[u8]; 5] = files!("off-black");
const OFF_WHITE: [&[u8]; 5] = files!("off-white");

/// The PNG bytes of one committed icon. A size not in `SIZES` gives the
/// next larger file, or the largest one.
pub fn png(state: State, shade: Shade, size: u32) -> &'static [u8] {
    let files = match (state, shade) {
        (State::On, Shade::Black) => ON_BLACK,
        (State::On, Shade::White) => ON_WHITE,
        (State::Off, Shade::Black) => OFF_BLACK,
        (State::Off, Shade::White) => OFF_WHITE,
    };
    let size = size_for(size, 1.0);
    let index = SIZES
        .iter()
        .position(|&each| each == size)
        .expect("size_for returns a member of SIZES");
    files[index]
}

/// The pixels of one decoded icon, 4 bytes per pixel (red, green, blue,
/// alpha), row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decodes PNG bytes to 8-bit RGBA, the form a tray icon takes.
pub fn rgba(png_bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "the image is too large".to_string())?;
    let mut buffer = vec![0; size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    let data = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("an indexed image was not expanded".to_string()),
    };
    Ok(Rgba {
        pixels,
        width: info.width,
        height: info.height,
    })
}

/// Reads the taskbar theme. Not unit-tested: it reads the real registry.
#[cfg(windows)]
mod windows_theme {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    use crate::wide;

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const VALUE: &str = "SystemUsesLightTheme";

    /// The DWORD `SystemUsesLightTheme`, or `None` when it cannot be read.
    pub fn system_uses_light_theme() -> Option<u32> {
        let (key, value) = (wide(KEY), wide(VALUE));
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: both names end in a zero, and `data` and `size` are valid
        // for writes of the sizes given.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_taskbar_gets_black_and_a_dark_one_white() {
        assert_eq!(shade_for_windows(Some(1)), Shade::Black);
        assert_eq!(shade_for_windows(Some(0)), Shade::White);
    }

    #[test]
    fn a_missing_theme_value_counts_as_dark() {
        assert_eq!(shade_for_windows(None), Shade::White);
    }

    #[test]
    fn macos_and_linux_use_a_fixed_shade() {
        #[cfg(target_os = "macos")]
        assert_eq!(shade_now(), Shade::Black);
        #[cfg(not(any(target_os = "macos", windows)))]
        assert_eq!(shade_now(), Shade::White);
        assert_eq!(TEMPLATE, cfg!(target_os = "macos"));
    }

    #[test]
    fn the_size_covers_the_scaled_points() {
        assert_eq!(size_for(16, 1.0), 16);
        assert_eq!(size_for(16, 1.25), 20);
        assert_eq!(size_for(16, 1.5), 24);
        assert_eq!(size_for(16, 2.0), 32);
        assert_eq!(size_for(16, 2.5), 48);
        assert_eq!(size_for(16, 4.0), 48);
        assert_eq!(size_for(18, 2.0), 48);
        assert_eq!(size_for(24, 1.0), 24);
        // Windows, 16 points.
        assert_eq!(size_for(16, 1.75), 32);
        assert_eq!(size_for(16, 3.0), 48);
        // macOS, 18 points.
        assert_eq!(size_for(18, 1.0), 20);
        assert_eq!(size_for(18, 1.5), 32);
        // Linux, 24 points: 60 pixels is more than any size, so the largest.
        assert_eq!(size_for(24, 2.5), 48);
    }

    #[test]
    fn each_choice_gives_its_own_file() {
        let mut seen = std::collections::HashSet::new();
        for state in [State::On, State::Off] {
            for shade in [Shade::Black, Shade::White] {
                for size in SIZES {
                    let bytes = png(state, shade, size);
                    assert_eq!(&bytes[1..4], b"PNG");
                    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), size);
                    assert!(seen.insert(bytes.as_ptr()));
                }
            }
        }
        // One anchor per table, so swapped arms in `png` fail.
        assert_eq!(
            png(State::On, Shade::Black, 16),
            include_bytes!("../icons/on-black-16.png")
        );
        assert_eq!(
            png(State::On, Shade::White, 20),
            include_bytes!("../icons/on-white-20.png")
        );
        assert_eq!(
            png(State::Off, Shade::Black, 32),
            include_bytes!("../icons/off-black-32.png")
        );
        assert_eq!(
            png(State::Off, Shade::White, 48),
            include_bytes!("../icons/off-white-48.png")
        );
    }

    #[test]
    fn every_icon_decodes_in_its_own_color() {
        for state in [State::On, State::Off] {
            for (shade, value) in [(Shade::Black, 0), (Shade::White, 255)] {
                for size in SIZES {
                    let image = rgba(png(state, shade, size)).unwrap();
                    assert_eq!((image.width, image.height), (size, size));
                    assert_eq!(image.pixels.len(), (size * size * 4) as usize);
                    let solid: Vec<&[u8]> = image
                        .pixels
                        .chunks_exact(4)
                        .filter(|pixel| pixel[3] == 255)
                        .collect();
                    assert!(!solid.is_empty(), "{state:?} {shade:?} {size}");
                    for pixel in solid {
                        assert_eq!(&pixel[..3], &[value; 3], "{state:?} {shade:?} {size}");
                    }
                }
            }
        }
    }
}
