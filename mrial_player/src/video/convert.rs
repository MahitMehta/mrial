pub struct RGBBuffer {
    rgb: Vec<u8>,
    width: usize,
    height: usize,
    expected_luma_size: usize,
}

impl RGBBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let expected_luma_size = width * height;

        let rval = Self {
            rgb: vec![0u8; 3 * expected_luma_size],
            width,
            height,
            expected_luma_size
        };

        rval
    }
    
    #[cfg(any(target_os = "windows"))]
    pub fn read_444_for_rgb8(&mut self, y: &[u8], u: &[u8], v: &[u8]) {
        use std::borrow::Borrow;

        use libyuv_sys::{
            kYvuF709Constants, // Windows | full range
            I444ToRGB24Matrix,
        };

        assert_eq!(y.len(), self.expected_luma_size);
        assert_eq!(u.len(), self.expected_luma_size);
        assert_eq!(v.len(), self.expected_luma_size);

        unsafe {
            I444ToRGB24Matrix(
                y.as_ptr(),
                self.width as _,
                v.as_ptr(),
                self.width as _,
                u.as_ptr(),
                self.width as _,
                self.rgb.as_mut_ptr(),
                (self.width * 3) as _,
                kYvuF709Constants.borrow(),
                self.width as _,
                self.height as _,
            );
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_444_for_rgb8(&mut self, y: &[u8], u: &[u8], v: &[u8]) {
        use std::borrow::Borrow;

        use libyuv_sys::{
            kYvuI601Constants, // Macos and Linux
            I444ToRGB24Matrix,
        };

        assert_eq!(y.len(), self.expected_luma_size);
        assert_eq!(u.len(), self.expected_luma_size);
        assert_eq!(v.len(), self.expected_luma_size);

        unsafe {
            I444ToRGB24Matrix(
                y.as_ptr(),
                self.width as _,
                v.as_ptr(),
                self.width as _,
                u.as_ptr(),
                self.width as _,
                self.rgb.as_mut_ptr(),
                (self.width * 3) as _,
                kYvuI601Constants.borrow(),
                self.width as _,
                self.height as _,
            );
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_420_for_rgb8(&mut self, y: &[u8], u: &[u8], v: &[u8]) {
        use std::borrow::Borrow;

        use libyuv_sys::{
            kYvuI601Constants, I420ToRGB24Matrix
        };

        assert_eq!(y.len(), self.expected_luma_size);
        assert_eq!(u.len(), self.expected_luma_size / 4);
        assert_eq!(v.len(), self.expected_luma_size / 4);

        unsafe {
            I420ToRGB24Matrix(
                y.as_ptr(),
                self.width as _,
                v.as_ptr(),
                (self.width / 2) as _,
                u.as_ptr(),
                (self.width / 2) as _,
                self.rgb.as_mut_ptr(),
                (self.width * 3) as _,
                kYvuI601Constants.borrow(),
                self.width as _,
                self.height as _,
            );
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.rgb
    }
}
